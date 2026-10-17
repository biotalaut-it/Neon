use super::{
    scanner::scan_workspace, symbols::extract_rust, DependencyHit, IndexLimits, IndexReport,
    KnowledgeHit, SymbolHit,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
};

pub struct KnowledgeIndex {
    conn: Connection,
}
impl KnowledgeIndex {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create knowledge database directory: {e}"))?;
        }
        let conn =
            Connection::open(path).map_err(|e| format!("cannot open knowledge database: {e}"))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")
            .map_err(|e| e.to_string())?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn reindex(&self, root: &Path, limits: &IndexLimits) -> Result<IndexReport, String> {
        let files = scan_workspace(root, limits)?;
        let old_paths = {
            let mut stmt = self
                .conn
                .prepare("SELECT path FROM knowledge_files")
                .map_err(|e| e.to_string())?;
            let paths = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<HashSet<_>, _>>()
                .map_err(|e| e.to_string())?;
            paths
        };
        let new_paths = files
            .iter()
            .map(|file| file.relative_path.to_string_lossy().into_owned())
            .collect::<HashSet<_>>();
        let removed_files = old_paths.difference(&new_paths).count();
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| format!("cannot begin knowledge index transaction: {e}"))?;
        let result = self.reindex_transaction(&files, removed_files);
        match result {
            Ok(report) => {
                self.conn
                    .execute_batch("COMMIT")
                    .map_err(|e| format!("cannot commit knowledge index: {e}"))?;
                Ok(report)
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    fn reindex_transaction(
        &self,
        files: &[super::ScannedFile],
        removed_files: usize,
    ) -> Result<IndexReport, String> {
        self.conn
            .execute("DELETE FROM knowledge_chunks", [])
            .map_err(|e| e.to_string())?;
        self.conn
            .execute("DELETE FROM symbols", [])
            .map_err(|e| e.to_string())?;
        self.conn
            .execute("DELETE FROM dependencies", [])
            .map_err(|e| e.to_string())?;
        self.conn
            .execute("DELETE FROM knowledge_files", [])
            .map_err(|e| e.to_string())?;
        let mut report = IndexReport {
            files_indexed: files.len(),
            removed_files,
            ..IndexReport::default()
        };
        for file in files {
            let relative = file.relative_path.to_string_lossy().into_owned();
            let language = file
                .relative_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("text")
                .to_ascii_lowercase();
            self.conn.execute("INSERT INTO knowledge_files(path,modified_at,byte_len,content_hash,language) VALUES(?1,?2,?3,?4,?5)",params![relative,file.modified_at,file.byte_len as i64,file.content_hash,language]).map_err(|e|format!("cannot insert indexed file: {e}"))?;
            let file_id = self.conn.last_insert_rowid();
            for (start, end, text) in split_chunks(&file.text, 1800) {
                self.conn.execute("INSERT INTO knowledge_chunks(file_id,start_line,end_line,text) VALUES(?1,?2,?3,?4)",params![file_id,start,end,text]).map_err(|e|format!("cannot insert knowledge chunk: {e}"))?;
                report.chunks_indexed += 1;
            }
            if language == "rs" {
                let (symbols, dependencies) = extract_rust(&file.text);
                for symbol in symbols {
                    self.conn.execute("INSERT INTO symbols(file_id,path,name,kind,signature,line) VALUES(?1,?2,?3,?4,?5,?6)",params![file_id,relative,symbol.name,symbol.kind,symbol.signature,symbol.line]).map_err(|e|format!("cannot insert Rust symbol: {e}"))?;
                    report.symbols_indexed += 1;
                }
                for dependency in dependencies {
                    self.conn.execute("INSERT INTO dependencies(file_id,from_path,to_name,kind,line) VALUES(?1,?2,?3,?4,?5)",params![file_id,relative,dependency.to_name,dependency.kind,dependency.line]).map_err(|e|format!("cannot insert Rust dependency: {e}"))?;
                    report.dependencies_indexed += 1;
                }
            }
            report.bytes_indexed = report.bytes_indexed.saturating_add(file.byte_len);
        }
        Ok(report)
    }

    pub fn search(
        &self,
        query: &str,
        max_hits: usize,
        max_bytes: usize,
    ) -> Result<Vec<KnowledgeHit>, String> {
        validate_search_limits(max_hits)?;
        let Some(expression) = fts_expression(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare("SELECT f.path,c.start_line,c.end_line,f.content_hash,c.text FROM knowledge_chunks_fts q JOIN knowledge_chunks c ON c.id=q.rowid JOIN knowledge_files f ON f.id=c.file_id WHERE knowledge_chunks_fts MATCH ?1 ORDER BY bm25(knowledge_chunks_fts),f.path,c.start_line LIMIT ?2").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map(params![expression, max_hits as i64], |row| {
                Ok(KnowledgeHit {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    start_line: row.get(1)?,
                    end_line: row.get(2)?,
                    content_hash: row.get(3)?,
                    text: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let hits = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut bytes = 0usize;
        let mut selected = Vec::new();
        for hit in hits {
            if hit.text.len().saturating_add(bytes) > max_bytes {
                continue;
            }
            bytes += hit.text.len();
            selected.push(hit);
        }
        Ok(selected)
    }

    pub fn search_symbols(&self, query: &str, max_hits: usize) -> Result<Vec<SymbolHit>, String> {
        validate_search_limits(max_hits)?;
        let Some(expression) = fts_expression(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare("SELECT s.path,s.name,s.kind,s.signature,s.line FROM symbols_fts q JOIN symbols s ON s.id=q.rowid WHERE symbols_fts MATCH ?1 ORDER BY bm25(symbols_fts),s.path,s.line LIMIT ?2").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map(params![expression, max_hits as i64], |row| {
                Ok(SymbolHit {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    signature: row.get(3)?,
                    line: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn search_dependencies(
        &self,
        query: &str,
        max_hits: usize,
    ) -> Result<Vec<DependencyHit>, String> {
        validate_search_limits(max_hits)?;
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare("SELECT from_path,to_name,kind,line FROM dependencies WHERE instr(lower(to_name),lower(?1))>0 ORDER BY from_path,line LIMIT ?2").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map(params![query.trim(), max_hits as i64], |row| {
                Ok(DependencyHit {
                    from_path: PathBuf::from(row.get::<_, String>(0)?),
                    to_name: row.get(1)?,
                    kind: row.get(2)?,
                    line: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn trace_dependency_graph(
        &self,
        query: &str,
        max_depth: usize,
        max_edges: usize,
    ) -> Result<Vec<super::DependencyGraphHit>, String> {
        if !(1..=8).contains(&max_depth) {
            return Err("knowledge graph depth must be in 1..=8".into());
        }
        if !(1..=200).contains(&max_edges) {
            return Err("knowledge graph edge limit must be in 1..=200".into());
        }
        let mut seeds = self
            .search_symbols(query, 20)?
            .into_iter()
            .map(|hit| hit.path)
            .collect::<HashSet<_>>();
        if seeds.is_empty() {
            seeds.extend(
                self.search_dependencies(query, 20)?
                    .into_iter()
                    .map(|hit| hit.from_path),
            );
        }
        let indexed_paths = {
            let mut statement = self
                .conn
                .prepare("SELECT path FROM knowledge_files")
                .map_err(|error| error.to_string())?;
            let paths = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| error.to_string())?
                .collect::<Result<HashSet<_>, _>>()
                .map_err(|error| error.to_string())?;
            paths
        };
        let mut queue = seeds
            .iter()
            .cloned()
            .map(|path| (path, 0usize))
            .collect::<VecDeque<_>>();
        let mut visited = seeds;
        let mut graph = Vec::new();
        while let Some((from_path, depth)) = queue.pop_front() {
            if depth >= max_depth || graph.len() >= max_edges {
                continue;
            }
            let mut statement = self
                .conn
                .prepare(
                    "SELECT to_name,kind,line FROM dependencies WHERE from_path=?1 ORDER BY line",
                )
                .map_err(|error| error.to_string())?;
            let dependencies = statement
                .query_map([from_path.to_string_lossy().as_ref()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u32>(2)?,
                    ))
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            for (target, kind, line) in dependencies {
                let resolved_path =
                    resolve_dependency_path(&from_path, &target, &kind, &indexed_paths);
                graph.push(super::DependencyGraphHit {
                    from_path: from_path.clone(),
                    target,
                    resolved_path: resolved_path.clone(),
                    kind,
                    line,
                    depth: depth + 1,
                });
                if graph.len() >= max_edges {
                    break;
                }
                if let Some(path) = resolved_path {
                    if depth + 1 < max_depth && visited.insert(path.clone()) {
                        queue.push_back((path, depth + 1));
                    }
                }
            }
        }
        Ok(graph)
    }
}

fn resolve_dependency_path(
    from_path: &Path,
    target: &str,
    kind: &str,
    indexed_paths: &HashSet<String>,
) -> Option<PathBuf> {
    let source_parent = from_path.parent().unwrap_or_else(|| Path::new(""));
    let mut segments = target
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|segment| !segment.is_empty() && *segment != "use")
        .collect::<Vec<_>>();
    let mut base = source_parent.to_path_buf();
    match segments.first().copied() {
        Some("crate") => {
            segments.remove(0);
        }
        Some("self") => {
            segments.remove(0);
        }
        Some("super") => {
            segments.remove(0);
            base.pop();
        }
        _ => {}
    }
    if kind == "mod" {
        segments.truncate(1);
    }
    for count in (1..=segments.len()).rev() {
        let module = segments[..count].join("/");
        let file_candidate = base.join(format!("{module}.rs"));
        if indexed_paths.contains(&file_candidate.to_string_lossy().into_owned()) {
            return Some(file_candidate);
        }
        let directory_candidate = base.join(&module).join("mod.rs");
        if indexed_paths.contains(&directory_candidate.to_string_lossy().into_owned()) {
            return Some(directory_candidate);
        }
    }
    None
}

fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS neon_schema_versions(component TEXT PRIMARY KEY,version INTEGER NOT NULL);").map_err(|e|e.to_string())?;
    let version: Option<i64> = conn
        .query_row(
            "SELECT version FROM neon_schema_versions WHERE component='knowledge'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if version.unwrap_or(0) < 1 {
        conn.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS knowledge_files(id INTEGER PRIMARY KEY,path TEXT NOT NULL UNIQUE,modified_at INTEGER NOT NULL,byte_len INTEGER NOT NULL,content_hash TEXT NOT NULL,language TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS knowledge_chunks(id INTEGER PRIMARY KEY,file_id INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,start_line INTEGER NOT NULL,end_line INTEGER NOT NULL,text TEXT NOT NULL);
            CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_fts USING fts5(text,path UNINDEXED,start_line UNINDEXED,end_line UNINDEXED);
            CREATE TRIGGER IF NOT EXISTS knowledge_chunks_ai AFTER INSERT ON knowledge_chunks BEGIN INSERT INTO knowledge_chunks_fts(rowid,text,path,start_line,end_line) SELECT new.id,new.text,f.path,new.start_line,new.end_line FROM knowledge_files f WHERE f.id=new.file_id; END;
            CREATE TRIGGER IF NOT EXISTS knowledge_chunks_ad AFTER DELETE ON knowledge_chunks BEGIN DELETE FROM knowledge_chunks_fts WHERE rowid=old.id; END;
            CREATE TABLE IF NOT EXISTS symbols(id INTEGER PRIMARY KEY,file_id INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,path TEXT NOT NULL,name TEXT NOT NULL,kind TEXT NOT NULL,signature TEXT NOT NULL,line INTEGER NOT NULL);
            CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(name,kind,signature,path UNINDEXED,line UNINDEXED);
            CREATE TRIGGER IF NOT EXISTS symbols_ai AFTER INSERT ON symbols BEGIN INSERT INTO symbols_fts(rowid,name,kind,signature,path,line) VALUES(new.id,new.name,new.kind,new.signature,new.path,new.line); END;
            CREATE TRIGGER IF NOT EXISTS symbols_ad AFTER DELETE ON symbols BEGIN DELETE FROM symbols_fts WHERE rowid=old.id; END;
            CREATE TABLE IF NOT EXISTS dependencies(id INTEGER PRIMARY KEY,file_id INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,from_path TEXT NOT NULL,to_name TEXT NOT NULL,kind TEXT NOT NULL,line INTEGER NOT NULL);
            INSERT INTO neon_schema_versions(component,version) VALUES('knowledge',1) ON CONFLICT(component) DO UPDATE SET version=excluded.version;
            COMMIT;").map_err(|e|format!("cannot migrate knowledge index schema: {e}"))?;
    }
    Ok(())
}

fn validate_search_limits(max_hits: usize) -> Result<(), String> {
    if max_hits == 0 || max_hits > 100 {
        return Err("knowledge search hit limit must be in 1..=100".into());
    }
    Ok(())
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
fn split_chunks(text: &str, max_bytes: usize) -> Vec<(u32, u32, String)> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut start_line = 1u32;
    let mut end_line = 1u32;
    for (index, line) in text.lines().enumerate() {
        let line_no = (index + 1).min(u32::MAX as usize) as u32;
        if line.len() > max_bytes {
            if !current.is_empty() {
                output.push((start_line, end_line, std::mem::take(&mut current)));
            }
            let mut piece = String::new();
            for ch in line.chars() {
                if piece.len() + ch.len_utf8() > max_bytes {
                    output.push((line_no, line_no, std::mem::take(&mut piece)));
                }
                piece.push(ch);
            }
            if !piece.is_empty() {
                output.push((line_no, line_no, piece));
            }
            start_line = line_no.saturating_add(1);
            end_line = start_line;
        } else {
            let add = line.len() + usize::from(!current.is_empty());
            if !current.is_empty() && current.len() + add > max_bytes {
                output.push((start_line, end_line, std::mem::take(&mut current)));
                start_line = line_no;
            }
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
            end_line = line_no;
        }
    }
    if !current.is_empty() {
        output.push((start_line, end_line, current));
    }
    output
}
