use crate::{tools::ToolRequest, verification::VerificationEvidence};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone)]
pub struct NewPlanStep {
    pub title: String,
    pub instruction: String,
    pub tool: ToolRequest,
    /// Zero-based ordinal references; dependencies must refer to earlier steps.
    pub depends_on: Vec<usize>,
    pub acceptance: String,
}
#[derive(Debug, Clone)]
pub struct PlanStep {
    pub id: i64,
    pub ordinal: u8,
    pub title: String,
    pub instruction: String,
    pub tool: ToolRequest,
    pub depends_on: Vec<i64>,
    pub acceptance: String,
    pub status: StepStatus,
    pub attempts: u8,
    pub error: Option<String>,
    pub evidence: Option<VerificationEvidence>,
}
#[derive(Debug, Clone)]
pub struct Plan {
    pub id: i64,
    pub goal: String,
    pub status: PlanStatus,
    pub steps: Vec<PlanStep>,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Active,
    Failed,
    Completed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Verified,
    Failed,
    Blocked,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanDraft {
    goal: String,
    steps: Vec<PlanDraftStep>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanDraftStep {
    title: String,
    instruction: String,
    tool: serde_json::Value,
    depends_on: Vec<usize>,
    acceptance: String,
}

pub fn parse_plan_draft(input: &str) -> Result<(String, Vec<NewPlanStep>), String> {
    if input.len() > 64 * 1024 {
        return Err("plan draft exceeds 64 KiB".into());
    }
    let draft: PlanDraft =
        serde_json::from_str(input).map_err(|e| format!("invalid strict plan JSON: {e}"))?;
    validate_text(&draft.goal, "plan goal", 2048)?;
    if !(2..=12).contains(&draft.steps.len()) {
        return Err("a task plan must contain 2..=12 steps".into());
    }
    let mut steps = Vec::with_capacity(draft.steps.len());
    for step in draft.steps {
        let tool_json = serde_json::to_string(&step.tool).map_err(|e| e.to_string())?;
        let tool = ToolRequest::parse_json(&tool_json)?;
        steps.push(NewPlanStep {
            title: step.title,
            instruction: step.instruction,
            tool,
            depends_on: step.depends_on,
            acceptance: step.acceptance,
        });
    }
    Ok((draft.goal, steps))
}

pub struct TaskStore {
    conn: Connection,
}
impl TaskStore {
    pub fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create task database directory: {e}"))?;
        }
        let conn =
            Connection::open(db_path).map_err(|e| format!("cannot open task database: {e}"))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")
            .map_err(|e| e.to_string())?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn create_plan(&mut self, goal: &str, steps: Vec<NewPlanStep>) -> Result<i64, String> {
        validate_text(goal, "plan goal", 2048)?;
        if !(2..=12).contains(&steps.len()) {
            return Err("a task plan must contain 2..=12 steps".into());
        }
        for (ordinal, step) in steps.iter().enumerate() {
            validate_text(&step.title, "step title", 128)?;
            validate_text(&step.instruction, "step instruction", 4096)?;
            validate_text(&step.acceptance, "step acceptance", 512)?;
            step.tool.validate()?;
            if step.tool.to_json()?.len() > 8 * 1024 {
                return Err("serialized tool request exceeds 8 KiB".into());
            }
            let mut seen = std::collections::HashSet::new();
            for dependency in &step.depends_on {
                if *dependency >= ordinal {
                    return Err(
                        "step dependencies must refer to unique earlier step ordinals".into(),
                    );
                }
                if !seen.insert(*dependency) {
                    return Err("duplicate step dependency".into());
                }
            }
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let now = now_epoch();
        tx.execute(
            "INSERT INTO plans(goal,status,created_at,updated_at) VALUES(?1,'active',?2,?2)",
            params![goal.trim(), now],
        )
        .map_err(|e| e.to_string())?;
        let plan_id = tx.last_insert_rowid();
        let mut ids: Vec<i64> = Vec::with_capacity(steps.len());
        for (ordinal, step) in steps.into_iter().enumerate() {
            let deps = step.depends_on.iter().map(|i| ids[*i]).collect::<Vec<_>>();
            let tool_json = step.tool.to_json()?;
            let deps_json = serde_json::to_string(&deps).map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO plan_steps(plan_id,ordinal,title,instruction,tool_json,depends_on_json,acceptance,status,attempts) VALUES(?1,?2,?3,?4,?5,?6,?7,'pending',0)",params![plan_id,ordinal as i64,step.title.trim(),step.instruction.trim(),tool_json,deps_json,step.acceptance.trim()]).map_err(|e|e.to_string())?;
            ids.push(tx.last_insert_rowid());
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(plan_id)
    }

    pub fn claim_next_step(&mut self, plan_id: i64) -> Result<Option<PlanStep>, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let status: Option<String> = tx
            .query_row("SELECT status FROM plans WHERE id=?1", [plan_id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        if status.as_deref() != Some("active") {
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(None);
        }
        let candidates = load_steps(&tx, plan_id)?;
        for step in candidates
            .into_iter()
            .filter(|step| step.status == StepStatus::Pending)
        {
            let mut ready = true;
            for dep in &step.depends_on {
                let status: Option<String> = tx
                    .query_row(
                        "SELECT status FROM plan_steps WHERE id=?1 AND plan_id=?2",
                        params![dep, plan_id],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?;
                if status.as_deref() != Some("verified") {
                    ready = false;
                    break;
                }
            }
            if !ready {
                continue;
            }
            let changed=tx.execute("UPDATE plan_steps SET status='running',attempts=attempts+1,error=NULL WHERE id=?1 AND status='pending'",[step.id]).map_err(|e|e.to_string())?;
            if changed == 0 {
                continue;
            }
            tx.execute(
                "UPDATE plans SET status='active',updated_at=?1 WHERE id=?2",
                params![now_epoch(), plan_id],
            )
            .map_err(|e| e.to_string())?;
            let result = load_step(&tx, step.id)?;
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(Some(result));
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(None)
    }

    pub fn finish_step(
        &mut self,
        step_id: i64,
        evidence: &VerificationEvidence,
    ) -> Result<(), String> {
        if evidence.status != crate::verification::VerificationStatus::Verified {
            return Err("only verified evidence can finish a plan step".into());
        }
        let encoded = serde_json::to_string(evidence).map_err(|e| e.to_string())?;
        if encoded.len() > 8 * 1024 {
            return Err("verification evidence exceeds 8 KiB".into());
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let plan_id: Option<i64> = tx
            .query_row(
                "SELECT plan_id FROM plan_steps WHERE id=?1 AND status='running'",
                [step_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let plan_id = plan_id.ok_or("plan step is not running")?;
        tx.execute(
            "UPDATE plan_steps SET status='verified',evidence_json=?1,error=NULL WHERE id=?2",
            params![encoded, step_id],
        )
        .map_err(|e| e.to_string())?;
        let unfinished: i64 = tx
            .query_row(
                "SELECT count(*) FROM plan_steps WHERE plan_id=?1 AND status!='verified'",
                [plan_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let status = if unfinished == 0 {
            "completed"
        } else {
            "active"
        };
        tx.execute(
            "UPDATE plans SET status=?1,updated_at=?2 WHERE id=?3",
            params![status, now_epoch(), plan_id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn fail_step(&mut self, step_id: i64, error: &str) -> Result<(), String> {
        validate_text(error, "step error", 2048)?;
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let plan_id: Option<i64> = tx
            .query_row(
                "SELECT plan_id FROM plan_steps WHERE id=?1 AND status='running'",
                [step_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let plan_id = plan_id.ok_or("plan step is not running")?;
        tx.execute(
            "UPDATE plan_steps SET status='failed',error=?1 WHERE id=?2",
            params![error, step_id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE plans SET status='failed',updated_at=?1 WHERE id=?2",
            params![now_epoch(), plan_id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn resume(&mut self, plan_id: i64) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                [plan_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !exists {
            return Err("plan id not found".into());
        }
        let exhausted:i64=tx.query_row("SELECT count(*) FROM plan_steps WHERE plan_id=?1 AND status IN ('failed','running') AND attempts>=2",[plan_id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if exhausted > 0 {
            return Err("plan step retry limit reached (one retry maximum)".into());
        }
        tx.execute("UPDATE plan_steps SET status='pending',error=CASE WHEN status='running' THEN 'interrupted before checkpoint' ELSE NULL END WHERE plan_id=?1 AND status IN ('failed','blocked','running')",[plan_id]).map_err(|e|e.to_string())?;
        tx.execute(
            "UPDATE plans SET status='active',updated_at=?1 WHERE id=?2",
            params![now_epoch(), plan_id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn load_plan(&self, plan_id: i64) -> Result<Plan, String> {
        let row: Option<(String, String, i64, i64)> = self
            .conn
            .query_row(
                "SELECT goal,status,created_at,updated_at FROM plans WHERE id=?1",
                [plan_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let (goal, status, created_at, updated_at) = row.ok_or("plan id not found")?;
        Ok(Plan {
            id: plan_id,
            goal,
            status: parse_plan_status(&status)?,
            steps: load_steps(&self.conn, plan_id)?,
            created_at,
            updated_at,
        })
    }

    pub fn list_plans(&self, limit: usize) -> Result<Vec<(i64, String, PlanStatus, i64)>, String> {
        if limit == 0 || limit > 100 {
            return Err("plan list limit must be in 1..=100".into());
        }
        let mut stmt=self.conn.prepare("SELECT id,goal,status,updated_at FROM plans ORDER BY updated_at DESC,id DESC LIMIT ?1").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map([limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.map(|row| {
            let (id, goal, status, updated) = row.map_err(|e| e.to_string())?;
            Ok((id, goal, parse_plan_status(&status)?, updated))
        })
        .collect()
    }
}

fn load_steps(conn: &Connection, plan_id: i64) -> Result<Vec<PlanStep>, String> {
    let mut stmt=conn.prepare("SELECT id,ordinal,title,instruction,tool_json,depends_on_json,acceptance,status,attempts,error,evidence_json FROM plan_steps WHERE plan_id=?1 ORDER BY ordinal").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([plan_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, Option<String>>(9)?,
                r.get::<_, Option<String>>(10)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    rows.map(|row| {
        let (
            id,
            ordinal,
            title,
            instruction,
            tool_json,
            deps_json,
            acceptance,
            status,
            attempts,
            error,
            evidence,
        ) = row.map_err(|e| e.to_string())?;
        let tool = ToolRequest::parse_json(&tool_json)?;
        let depends_on = serde_json::from_str(&deps_json)
            .map_err(|e| format!("invalid stored dependencies: {e}"))?;
        let evidence = evidence
            .map(|raw| {
                serde_json::from_str(&raw).map_err(|e| format!("invalid stored evidence: {e}"))
            })
            .transpose()?;
        Ok(PlanStep {
            id,
            ordinal: u8::try_from(ordinal).map_err(|_| "invalid stored ordinal")?,
            title,
            instruction,
            tool,
            depends_on,
            acceptance,
            status: parse_step_status(&status)?,
            attempts: u8::try_from(attempts).unwrap_or(u8::MAX),
            error,
            evidence,
        })
    })
    .collect()
}
fn load_step(conn: &Connection, id: i64) -> Result<PlanStep, String> {
    let plan_id: i64 = conn
        .query_row("SELECT plan_id FROM plan_steps WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    load_steps(conn, plan_id)?
        .into_iter()
        .find(|step| step.id == id)
        .ok_or("plan step not found".into())
}
fn parse_plan_status(s: &str) -> Result<PlanStatus, String> {
    match s {
        "active" => Ok(PlanStatus::Active),
        "failed" => Ok(PlanStatus::Failed),
        "completed" => Ok(PlanStatus::Completed),
        _ => Err(format!("unknown stored plan status: {s}")),
    }
}
fn parse_step_status(s: &str) -> Result<StepStatus, String> {
    match s {
        "pending" => Ok(StepStatus::Pending),
        "running" => Ok(StepStatus::Running),
        "verified" => Ok(StepStatus::Verified),
        "failed" => Ok(StepStatus::Failed),
        "blocked" => Ok(StepStatus::Blocked),
        _ => Err(format!("unknown stored step status: {s}")),
    }
}
fn validate_text(v: &str, label: &str, max: usize) -> Result<(), String> {
    if v.trim().is_empty() || v.len() > max {
        Err(format!("{label} must contain 1..={max} bytes"))
    } else {
        Ok(())
    }
}
fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS neon_schema_versions(component TEXT PRIMARY KEY,version INTEGER NOT NULL);").map_err(|e|e.to_string())?;
    let version: Option<i64> = conn
        .query_row(
            "SELECT version FROM neon_schema_versions WHERE component='planner'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if version.unwrap_or(0) < 1 {
        conn.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS plans(id INTEGER PRIMARY KEY,goal TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN ('active','failed','completed')),created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS plan_steps(id INTEGER PRIMARY KEY,plan_id INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,title TEXT NOT NULL,instruction TEXT NOT NULL,tool_json TEXT NOT NULL,depends_on_json TEXT NOT NULL,acceptance TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN ('pending','running','verified','failed','blocked')),attempts INTEGER NOT NULL DEFAULT 0,error TEXT,evidence_json TEXT,UNIQUE(plan_id,ordinal));
            CREATE INDEX IF NOT EXISTS plan_steps_status_idx ON plan_steps(plan_id,status,ordinal);
            INSERT INTO neon_schema_versions(component,version) VALUES('planner',1) ON CONFLICT(component) DO UPDATE SET version=excluded.version;
            COMMIT;").map_err(|e|format!("cannot migrate planner schema: {e}"))?;
    }
    Ok(())
}
