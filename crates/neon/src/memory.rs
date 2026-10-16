use rusqlite::{params, Connection};
use std::{
    collections::HashMap,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_RECORD_BYTES: usize = 8192;
const MAX_LIST_LIMIT: usize = 100;

#[derive(Debug, Clone)]
pub struct Memory {
    pub id: i64,
    pub category: String,
    pub text: String,
    pub created_at: i64,
}
#[derive(Debug, Clone)]
pub struct MemoryProposal {
    pub id: String,
    pub category: String,
    pub text: String,
}
#[derive(Debug, Clone)]
pub struct SummaryProposal {
    pub id: String,
    pub project: String,
    pub text: String,
}
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: i64,
    pub project: String,
    pub text: String,
    pub created_at: i64,
}
#[derive(Debug, Clone)]
pub struct ExperienceProposal {
    pub id: String,
    pub task: String,
    pub outcome: String,
    pub cause: String,
    pub solution: String,
    pub verification: String,
}
#[derive(Debug, Clone)]
pub struct Experience {
    pub id: i64,
    pub task: String,
    pub outcome: String,
    pub cause: String,
    pub solution: String,
    pub verification: String,
    pub created_at: i64,
    /// Stored experience is historical evidence, never proof about a new run.
    pub historical: bool,
}
#[derive(Debug, Clone)]
pub struct SkillProposal {
    pub id: String,
    pub name: String,
    pub triggers: String,
    pub procedure: String,
}
#[derive(Debug, Clone)]
pub struct Skill {
    pub id: i64,
    pub name: String,
    pub triggers: String,
    pub procedure: String,
    pub created_at: i64,
    pub builtin: bool,
}
#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub kind: String,
    pub source_id: i64,
    pub category: String,
    pub text: String,
    pub created_at: i64,
}

pub struct MemoryStore {
    conn: Connection,
    pending: HashMap<String, (String, String)>,
    pending_summaries: HashMap<String, (String, String)>,
    pending_experiences: HashMap<String, ExperienceProposal>,
    pending_skills: HashMap<String, SkillProposal>,
}
impl MemoryStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create memory directory: {e}"))?;
        }
        let conn = Connection::open(path)
            .map_err(|e| format!("cannot open local memory database: {e}"))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("cannot configure local memory database: {e}"))?;
        migrate(&conn)?;
        seed_builtin_skills(&conn)?;
        Ok(Self {
            conn,
            pending: HashMap::new(),
            pending_summaries: HashMap::new(),
            pending_experiences: HashMap::new(),
            pending_skills: HashMap::new(),
        })
    }

    pub fn schema_version(&self) -> Result<i64, String> {
        self.conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| e.to_string())
    }

    pub fn propose(&mut self, category: &str, text: &str) -> Result<MemoryProposal, String> {
        let text = text.trim();
        let category = category.trim();
        if text.is_empty() || category.is_empty() {
            return Err("memory category and text must not be empty".into());
        }
        if text.len() > MAX_RECORD_BYTES || category.len() > 128 {
            return Err("memory text must be <=8 KiB and category <=128 bytes".into());
        }
        let id = new_id()?;
        self.pending
            .insert(id.clone(), (category.to_string(), text.to_string()));
        Ok(MemoryProposal {
            id,
            category: category.to_string(),
            text: text.to_string(),
        })
    }

    pub fn approve(&mut self, id: &str) -> Result<i64, String> {
        let (category, text) = self
            .pending
            .remove(id)
            .ok_or("memory proposal not found or already processed")?;
        let now = now_epoch()?;
        self.conn
            .execute(
                "INSERT INTO memories(category,text,created_at) VALUES(?1,?2,?3)",
                params![category, text, now],
            )
            .map_err(|e| format!("cannot save memory: {e}"))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Memory>, String> {
        validate_limit(limit)?;
        let Some(expression) = fts_expression(query) else {
            return self.list(limit);
        };
        let mut stmt = self.conn.prepare("SELECT m.id,m.category,m.text,m.created_at FROM memories_fts f JOIN memories m ON m.id=f.rowid WHERE memories_fts MATCH ?1 ORDER BY bm25(memories_fts),m.created_at DESC LIMIT ?2").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![expression, limit as i64], |r| {
                Ok(Memory {
                    id: r.get(0)?,
                    category: r.get(1)?,
                    text: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn list(&self, limit: usize) -> Result<Vec<Memory>, String> {
        validate_limit(limit)?;
        let mut stmt = self.conn.prepare("SELECT id,category,text,created_at FROM memories ORDER BY created_at DESC,id DESC LIMIT ?1").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([limit as i64], |r| {
                Ok(Memory {
                    id: r.get(0)?,
                    category: r.get(1)?,
                    text: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn update(&mut self, id: i64, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() || text.len() > MAX_RECORD_BYTES {
            return Err("updated memory must contain 1..=8192 bytes".into());
        }
        let n = self
            .conn
            .execute("UPDATE memories SET text=?1 WHERE id=?2", params![text, id])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("memory id not found".into());
        }
        Ok(())
    }

    pub fn forget(&mut self, id: i64) -> Result<(), String> {
        let n = self
            .conn
            .execute("DELETE FROM memories WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("memory id not found".into());
        }
        Ok(())
    }

    pub fn forget_all(&mut self) -> Result<(), String> {
        self.pending.clear();
        self.conn
            .execute("DELETE FROM memories", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn propose_summary(
        &mut self,
        project: &str,
        text: &str,
    ) -> Result<SummaryProposal, String> {
        let project = project.trim();
        let text = text.trim();
        validate_text(project, "summary project", 128)?;
        validate_text(text, "summary text", MAX_RECORD_BYTES)?;
        let id = new_id()?;
        self.pending_summaries
            .insert(id.clone(), (project.to_string(), text.to_string()));
        Ok(SummaryProposal {
            id,
            project: project.to_string(),
            text: text.to_string(),
        })
    }

    pub fn approve_summary(&mut self, id: &str) -> Result<i64, String> {
        let (project, text) = self
            .pending_summaries
            .remove(id)
            .ok_or("summary proposal not found or already processed")?;
        self.conn
            .execute(
                "INSERT INTO session_summaries(project,text,created_at) VALUES(?1,?2,?3)",
                params![project, text, now_epoch()?],
            )
            .map_err(|e| e.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn search_summaries(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SessionSummary>, String> {
        validate_limit(limit)?;
        let Some(expression) = fts_expression(query) else {
            return self.list_summaries(limit);
        };
        let mut stmt = self.conn.prepare("SELECT s.id,s.project,s.text,s.created_at FROM session_summaries_fts f JOIN session_summaries s ON s.id=f.rowid WHERE session_summaries_fts MATCH ?1 ORDER BY bm25(session_summaries_fts),s.created_at DESC LIMIT ?2").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![expression, limit as i64], |r| {
                Ok(SessionSummary {
                    id: r.get(0)?,
                    project: r.get(1)?,
                    text: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn list_summaries(&self, limit: usize) -> Result<Vec<SessionSummary>, String> {
        validate_limit(limit)?;
        let mut stmt = self.conn.prepare("SELECT id,project,text,created_at FROM session_summaries ORDER BY created_at DESC,id DESC LIMIT ?1").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([limit as i64], |r| {
                Ok(SessionSummary {
                    id: r.get(0)?,
                    project: r.get(1)?,
                    text: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn update_summary(&mut self, id: i64, text: &str) -> Result<(), String> {
        validate_text(text.trim(), "summary text", MAX_RECORD_BYTES)?;
        let n = self
            .conn
            .execute(
                "UPDATE session_summaries SET text=?1 WHERE id=?2",
                params![text.trim(), id],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("summary id not found".into());
        }
        Ok(())
    }

    pub fn delete_summary(&mut self, id: i64) -> Result<(), String> {
        let n = self
            .conn
            .execute("DELETE FROM session_summaries WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("summary id not found".into());
        }
        Ok(())
    }

    pub fn propose_experience(
        &mut self,
        task: &str,
        outcome: &str,
        cause: &str,
        solution: &str,
        verification: &str,
    ) -> Result<ExperienceProposal, String> {
        for (value, label) in [
            (task, "experience task"),
            (outcome, "experience outcome"),
            (cause, "experience cause"),
            (solution, "experience solution"),
            (verification, "experience verification"),
        ] {
            validate_text(value.trim(), label, MAX_RECORD_BYTES)?;
        }
        let p = ExperienceProposal {
            id: new_id()?,
            task: task.trim().to_string(),
            outcome: outcome.trim().to_string(),
            cause: cause.trim().to_string(),
            solution: solution.trim().to_string(),
            verification: verification.trim().to_string(),
        };
        self.pending_experiences.insert(p.id.clone(), p.clone());
        Ok(p)
    }

    pub fn approve_experience(&mut self, id: &str) -> Result<i64, String> {
        let p = self
            .pending_experiences
            .remove(id)
            .ok_or("experience proposal not found or already processed")?;
        self.conn.execute("INSERT INTO experiences(task,outcome,cause,solution,verification,created_at) VALUES(?1,?2,?3,?4,?5,?6)", params![p.task,p.outcome,p.cause,p.solution,p.verification,now_epoch()?]).map_err(|e| e.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn search_experiences(&self, query: &str, limit: usize) -> Result<Vec<Experience>, String> {
        validate_limit(limit)?;
        let Some(expression) = fts_expression(query) else {
            return self.list_experiences(limit);
        };
        let mut stmt = self.conn.prepare("SELECT e.id,e.task,e.outcome,e.cause,e.solution,e.verification,e.created_at FROM experiences_fts f JOIN experiences e ON e.id=f.rowid WHERE experiences_fts MATCH ?1 ORDER BY bm25(experiences_fts),e.created_at DESC LIMIT ?2").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![expression, limit as i64], |r| {
                experience_from_row(r)
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn list_experiences(&self, limit: usize) -> Result<Vec<Experience>, String> {
        validate_limit(limit)?;
        let mut stmt = self.conn.prepare("SELECT id,task,outcome,cause,solution,verification,created_at FROM experiences ORDER BY created_at DESC,id DESC LIMIT ?1").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([limit as i64], experience_from_row)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn delete_experience(&mut self, id: i64) -> Result<(), String> {
        let n = self
            .conn
            .execute("DELETE FROM experiences WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("experience id not found".into());
        }
        Ok(())
    }

    pub fn propose_skill(
        &mut self,
        name: &str,
        triggers: &str,
        procedure: &str,
    ) -> Result<SkillProposal, String> {
        validate_text(name.trim(), "skill name", 128)?;
        validate_text(triggers.trim(), "skill triggers", 1024)?;
        validate_text(procedure.trim(), "skill procedure", MAX_RECORD_BYTES)?;
        let p = SkillProposal {
            id: new_id()?,
            name: name.trim().to_string(),
            triggers: triggers.trim().to_string(),
            procedure: procedure.trim().to_string(),
        };
        self.pending_skills.insert(p.id.clone(), p.clone());
        Ok(p)
    }

    pub fn approve_skill(&mut self, id: &str) -> Result<i64, String> {
        let p = self
            .pending_skills
            .remove(id)
            .ok_or("skill proposal not found or already processed")?;
        self.conn.execute("INSERT INTO skills(name,triggers,procedure,created_at,builtin) VALUES(?1,?2,?3,?4,0)", params![p.name,p.triggers,p.procedure,now_epoch()?]).map_err(|e| format!("cannot save skill: {e}"))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_skills(&self, limit: usize) -> Result<Vec<Skill>, String> {
        validate_limit(limit)?;
        let mut stmt = self.conn.prepare("SELECT id,name,triggers,procedure,created_at,builtin FROM skills ORDER BY builtin DESC,name LIMIT ?1").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map([limit as i64], |r| {
                Ok(Skill {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    triggers: r.get(2)?,
                    procedure: r.get(3)?,
                    created_at: r.get(4)?,
                    builtin: r.get::<_, i64>(5)? != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn search_skills(&self, query: &str, limit: usize) -> Result<Vec<Skill>, String> {
        validate_limit(limit)?;
        let Some(expression) = fts_expression(query) else {
            return self.list_skills(limit);
        };
        let mut stmt = self.conn.prepare("SELECT s.id,s.name,s.triggers,s.procedure,s.created_at,s.builtin FROM skills_fts f JOIN skills s ON s.id=f.rowid WHERE skills_fts MATCH ?1 ORDER BY bm25(skills_fts),s.builtin DESC,s.name LIMIT ?2").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map(params![expression, limit as i64], |r| {
                Ok(Skill {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    triggers: r.get(2)?,
                    procedure: r.get(3)?,
                    created_at: r.get(4)?,
                    builtin: r.get::<_, i64>(5)? != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn delete_skill(&mut self, id: i64) -> Result<(), String> {
        let n = self
            .conn
            .execute("DELETE FROM skills WHERE id=?1 AND builtin=0", [id])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("user skill not found (built-in skills cannot be deleted)".into());
        }
        Ok(())
    }

    pub fn update_skill(
        &mut self,
        id: i64,
        name: &str,
        triggers: &str,
        procedure: &str,
    ) -> Result<(), String> {
        validate_text(name.trim(), "skill name", 128)?;
        validate_text(triggers.trim(), "skill triggers", 1024)?;
        validate_text(procedure.trim(), "skill procedure", MAX_RECORD_BYTES)?;
        let changed = self
            .conn
            .execute(
                "UPDATE skills SET name=?1,triggers=?2,procedure=?3 WHERE id=?4 AND builtin=0",
                params![name.trim(), triggers.trim(), procedure.trim(), id],
            )
            .map_err(|e| format!("cannot update skill: {e}"))?;
        if changed == 0 {
            return Err("user skill not found (built-in skills cannot be edited)".into());
        }
        Ok(())
    }

    pub fn retrieve_context(
        &self,
        query: &str,
        limit: usize,
        max_bytes: usize,
    ) -> Result<Vec<ContextEntry>, String> {
        validate_limit(limit)?;
        if query.trim().is_empty() || max_bytes == 0 {
            return Ok(Vec::new());
        }
        let mut candidates = Vec::new();
        for m in self.search(query, limit)? {
            candidates.push(ContextEntry {
                kind: "memory".into(),
                source_id: m.id,
                category: m.category,
                text: m.text,
                created_at: m.created_at,
            });
        }
        for s in self.search_summaries(query, limit)? {
            candidates.push(ContextEntry {
                kind: "summary".into(),
                source_id: s.id,
                category: s.project,
                text: s.text,
                created_at: s.created_at,
            });
        }
        for e in self.search_experiences(query, limit)? {
            let text = format!(
                "Task: {}\nOutcome: {}\nCause: {}\nSolution: {}\nHistorical verification: {}",
                e.task, e.outcome, e.cause, e.solution, e.verification
            );
            candidates.push(ContextEntry {
                kind: "experience".into(),
                source_id: e.id,
                category: e.outcome.clone(),
                text,
                created_at: e.created_at,
            });
        }
        for s in self.search_skills(query, limit)? {
            let text = format!("Triggers: {}\nProcedure: {}", s.triggers, s.procedure);
            candidates.push(ContextEntry {
                kind: "skill".into(),
                source_id: s.id,
                category: s.name,
                text,
                created_at: s.created_at,
            });
        }
        let mut used = 0usize;
        let mut output = Vec::new();
        for item in candidates {
            if output.len() >= limit {
                break;
            }
            let cost = item
                .text
                .len()
                .saturating_add(item.kind.len())
                .saturating_add(item.category.len());
            if used.saturating_add(cost) > max_bytes {
                continue;
            }
            used += cost;
            output.push(item);
        }
        Ok(output)
    }
}

fn migrate(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if version < 1 {
        conn.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS memories(id INTEGER PRIMARY KEY, category TEXT NOT NULL, text TEXT NOT NULL, created_at INTEGER NOT NULL);
            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(text, category, content='memories', content_rowid='id');
            INSERT INTO memories_fts(memories_fts) VALUES('rebuild');
            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN INSERT INTO memories_fts(rowid,text,category) VALUES(new.id,new.text,new.category); END;
            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN INSERT INTO memories_fts(memories_fts,rowid,text,category) VALUES('delete',old.id,old.text,old.category); END;
            CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN INSERT INTO memories_fts(memories_fts,rowid,text,category) VALUES('delete',old.id,old.text,old.category); INSERT INTO memories_fts(rowid,text,category) VALUES(new.id,new.text,new.category); END;
            PRAGMA user_version=1; COMMIT;").map_err(|e| format!("cannot migrate memory schema v1: {e}"))?;
    }
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if current < 2 {
        conn.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS session_summaries(id INTEGER PRIMARY KEY, project TEXT NOT NULL, text TEXT NOT NULL, created_at INTEGER NOT NULL);
            CREATE VIRTUAL TABLE IF NOT EXISTS session_summaries_fts USING fts5(project,text,content='session_summaries',content_rowid='id');
            CREATE TRIGGER IF NOT EXISTS summaries_ai AFTER INSERT ON session_summaries BEGIN INSERT INTO session_summaries_fts(rowid,project,text) VALUES(new.id,new.project,new.text); END;
            CREATE TRIGGER IF NOT EXISTS summaries_ad AFTER DELETE ON session_summaries BEGIN INSERT INTO session_summaries_fts(session_summaries_fts,rowid,project,text) VALUES('delete',old.id,old.project,old.text); END;
            CREATE TRIGGER IF NOT EXISTS summaries_au AFTER UPDATE ON session_summaries BEGIN INSERT INTO session_summaries_fts(session_summaries_fts,rowid,project,text) VALUES('delete',old.id,old.project,old.text); INSERT INTO session_summaries_fts(rowid,project,text) VALUES(new.id,new.project,new.text); END;
            CREATE TABLE IF NOT EXISTS experiences(id INTEGER PRIMARY KEY, task TEXT NOT NULL, outcome TEXT NOT NULL, cause TEXT NOT NULL, solution TEXT NOT NULL, verification TEXT NOT NULL, created_at INTEGER NOT NULL);
            CREATE VIRTUAL TABLE IF NOT EXISTS experiences_fts USING fts5(task,outcome,cause,solution,verification,content='experiences',content_rowid='id');
            CREATE TRIGGER IF NOT EXISTS experiences_ai AFTER INSERT ON experiences BEGIN INSERT INTO experiences_fts(rowid,task,outcome,cause,solution,verification) VALUES(new.id,new.task,new.outcome,new.cause,new.solution,new.verification); END;
            CREATE TRIGGER IF NOT EXISTS experiences_ad AFTER DELETE ON experiences BEGIN INSERT INTO experiences_fts(experiences_fts,rowid,task,outcome,cause,solution,verification) VALUES('delete',old.id,old.task,old.outcome,old.cause,old.solution,old.verification); END;
            CREATE TRIGGER IF NOT EXISTS experiences_au AFTER UPDATE ON experiences BEGIN INSERT INTO experiences_fts(experiences_fts,rowid,task,outcome,cause,solution,verification) VALUES('delete',old.id,old.task,old.outcome,old.cause,old.solution,old.verification); INSERT INTO experiences_fts(rowid,task,outcome,cause,solution,verification) VALUES(new.id,new.task,new.outcome,new.cause,new.solution,new.verification); END;
            CREATE TABLE IF NOT EXISTS skills(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, triggers TEXT NOT NULL, procedure TEXT NOT NULL, created_at INTEGER NOT NULL, builtin INTEGER NOT NULL CHECK(builtin IN (0,1)));
            CREATE VIRTUAL TABLE IF NOT EXISTS skills_fts USING fts5(name,triggers,procedure,content='skills',content_rowid='id');
            CREATE TRIGGER IF NOT EXISTS skills_ai AFTER INSERT ON skills BEGIN INSERT INTO skills_fts(rowid,name,triggers,procedure) VALUES(new.id,new.name,new.triggers,new.procedure); END;
            CREATE TRIGGER IF NOT EXISTS skills_ad AFTER DELETE ON skills BEGIN INSERT INTO skills_fts(skills_fts,rowid,name,triggers,procedure) VALUES('delete',old.id,old.name,old.triggers,old.procedure); END;
            CREATE TRIGGER IF NOT EXISTS skills_au AFTER UPDATE ON skills BEGIN INSERT INTO skills_fts(skills_fts,rowid,name,triggers,procedure) VALUES('delete',old.id,old.name,old.triggers,old.procedure); INSERT INTO skills_fts(rowid,name,triggers,procedure) VALUES(new.id,new.name,new.triggers,new.procedure); END;
            PRAGMA user_version=2; COMMIT;").map_err(|e| format!("cannot migrate memory schema v2: {e}"))?;
    }
    Ok(())
}

fn seed_builtin_skills(conn: &Connection) -> Result<(), String> {
    let skills = [
        ("debugging", "debug bug error panic regression", "1. Reproduce the failure with the smallest input.\n2. Read the full error and identify the failing location.\n3. Trace the value/control flow to a root cause.\n4. Make the smallest correction.\n5. Add a regression test and run the relevant test suite."),
        ("rust-test", "Rust cargo tests compile unit integration", "1. Inspect Cargo.toml and affected crate.\n2. Run cargo fmt --all -- --check.\n3. Run cargo test -p <affected-crate>.\n4. Run cargo clippy -p <affected-crate> --all-targets -- -D warnings.\n5. Report actual command exit status and failing test names."),
        ("repository-analysis", "repository codebase architecture symbols dependencies", "1. Read the top-level README and Cargo workspace manifest.\n2. List source and test files without reading build outputs.\n3. Search symbols and imports related to the request.\n4. Trace callers and tests before proposing edits.\n5. Cite exact relative paths and line ranges."),
        ("diff-review", "review diff patch code change security regression", "1. Read the complete diff and surrounding source.\n2. Check behavior, error paths, resource bounds, and permissions.\n3. Run focused tests and formatting checks.\n4. Report only actionable findings with file/line evidence.\n5. Separate verified results from untested risks."),
    ];
    for (name, triggers, procedure) in skills {
        conn.execute("INSERT OR IGNORE INTO skills(name,triggers,procedure,created_at,builtin) VALUES(?1,?2,?3,?4,1)",params![name,triggers,procedure,0i64]).map_err(|e|format!("cannot seed built-in skill: {e}"))?;
    }
    Ok(())
}

fn experience_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Experience> {
    Ok(Experience {
        id: row.get(0)?,
        task: row.get(1)?,
        outcome: row.get(2)?,
        cause: row.get(3)?,
        solution: row.get(4)?,
        verification: row.get(5)?,
        created_at: row.get(6)?,
        historical: true,
    })
}
fn validate_limit(limit: usize) -> Result<(), String> {
    if limit == 0 || limit > MAX_LIST_LIMIT {
        return Err("result limit must be in 1..=100".into());
    }
    Ok(())
}
fn validate_text(value: &str, label: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{label} must contain 1..={max} bytes"));
    }
    Ok(())
}
fn new_id() -> Result<String, String> {
    Ok(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ))
}
fn now_epoch() -> Result<i64, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64)
}
fn fts_expression(query: &str) -> Option<String> {
    let terms = query
        .split_whitespace()
        .filter_map(|term| {
            let clean = term.replace('"', "");
            (!clean.is_empty()).then(|| format!("\"{clean}\""))
        })
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}
