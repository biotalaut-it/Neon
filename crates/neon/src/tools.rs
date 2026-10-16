use crate::{
    knowledge::KnowledgeIndex,
    lab::{ChangeDiff, Workspace},
};
use neonatom::{eval, parse, Limits as AtomLimits, Value as AtomValue};
use serde::{de::DeserializeOwned, Deserialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_REQUEST_BYTES: usize = 8 * 1024;
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub enum ToolRequest {
    WorkspaceList,
    WorkspaceRead { path: String },
    WorkspaceSearch { query: String },
    SymbolsSearch { query: String },
    DependenciesSearch { query: String },
    GitStatus,
    GitDiff,
    NeonAtomEval { source: String },
    DuckDuckGoSearch { query: String },
    WorkspaceProposeWrite { path: String, content: String },
    CargoTest,
    CargoCheck,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    tool: String,
    arguments: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryArgs {
    query: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceArgs {
    source: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    path: String,
    content: String,
}

impl ToolRequest {
    pub fn parse_json(input: &str) -> Result<Self, String> {
        if input.len() > MAX_REQUEST_BYTES {
            return Err("tool JSON exceeds 8 KiB".into());
        }
        let envelope: Envelope =
            serde_json::from_str(input).map_err(|e| format!("invalid tool envelope: {e}"))?;
        let request = match envelope.tool.as_str() {
            "workspace_list" => {
                let _: EmptyArgs = parse_args(envelope.arguments)?;
                Self::WorkspaceList
            }
            "workspace_read" => {
                let args: PathArgs = parse_args(envelope.arguments)?;
                Self::WorkspaceRead { path: args.path }
            }
            "workspace_search" => {
                let args: QueryArgs = parse_args(envelope.arguments)?;
                Self::WorkspaceSearch { query: args.query }
            }
            "symbols_search" => {
                let args: QueryArgs = parse_args(envelope.arguments)?;
                Self::SymbolsSearch { query: args.query }
            }
            "dependencies_search" => {
                let args: QueryArgs = parse_args(envelope.arguments)?;
                Self::DependenciesSearch { query: args.query }
            }
            "git_status" => {
                let _: EmptyArgs = parse_args(envelope.arguments)?;
                Self::GitStatus
            }
            "git_diff" => {
                let _: EmptyArgs = parse_args(envelope.arguments)?;
                Self::GitDiff
            }
            "neonatom_eval" => {
                let args: SourceArgs = parse_args(envelope.arguments)?;
                Self::NeonAtomEval {
                    source: args.source,
                }
            }
            "duckduckgo_search" => {
                let args: QueryArgs = parse_args(envelope.arguments)?;
                Self::DuckDuckGoSearch { query: args.query }
            }
            "workspace_propose_write" => {
                let args: WriteArgs = parse_args(envelope.arguments)?;
                Self::WorkspaceProposeWrite {
                    path: args.path,
                    content: args.content,
                }
            }
            "cargo_test" => {
                let _: EmptyArgs = parse_args(envelope.arguments)?;
                Self::CargoTest
            }
            "cargo_check" => {
                let _: EmptyArgs = parse_args(envelope.arguments)?;
                Self::CargoCheck
            }
            name => return Err(format!("unknown tool: {name}")),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn to_json(&self) -> Result<String, String> {
        let (tool, arguments) = match self {
            Self::WorkspaceList => ("workspace_list", serde_json::json!({})),
            Self::WorkspaceRead { path } => ("workspace_read", serde_json::json!({"path":path})),
            Self::WorkspaceSearch { query } => {
                ("workspace_search", serde_json::json!({"query":query}))
            }
            Self::SymbolsSearch { query } => ("symbols_search", serde_json::json!({"query":query})),
            Self::DependenciesSearch { query } => {
                ("dependencies_search", serde_json::json!({"query":query}))
            }
            Self::GitStatus => ("git_status", serde_json::json!({})),
            Self::GitDiff => ("git_diff", serde_json::json!({})),
            Self::NeonAtomEval { source } => {
                ("neonatom_eval", serde_json::json!({"source":source}))
            }
            Self::DuckDuckGoSearch { query } => {
                ("duckduckgo_search", serde_json::json!({"query":query}))
            }
            Self::WorkspaceProposeWrite { path, content } => (
                "workspace_propose_write",
                serde_json::json!({"path":path,"content":content}),
            ),
            Self::CargoTest => ("cargo_test", serde_json::json!({})),
            Self::CargoCheck => ("cargo_check", serde_json::json!({})),
        };
        let json = serde_json::json!({"tool":tool,"arguments":arguments}).to_string();
        if json.len() > MAX_REQUEST_BYTES {
            return Err("tool JSON exceeds 8 KiB".into());
        }
        Ok(json)
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::WorkspaceRead { path } | Self::WorkspaceProposeWrite { path, .. } => {
                validate_text(path, "path", 1024)?;
                let p = Path::new(path);
                if p.is_absolute()
                    || p.components().any(|c| {
                        matches!(
                            c,
                            std::path::Component::ParentDir
                                | std::path::Component::RootDir
                                | std::path::Component::Prefix(_)
                        )
                    })
                {
                    return Err("tool path must be a safe relative workspace path".into());
                }
                if let Self::WorkspaceProposeWrite { content, .. } = self {
                    if content.len() > 1024 * 1024 {
                        return Err("write content exceeds 1 MiB".into());
                    }
                }
            }
            Self::WorkspaceSearch { query }
            | Self::SymbolsSearch { query }
            | Self::DependenciesSearch { query }
            | Self::DuckDuckGoSearch { query } => validate_text(query, "query", 2048)?,
            Self::NeonAtomEval { source } => validate_text(source, "NeonAtom source", 4096)?,
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolEffect {
    ReadOnly,
    NeedsApproval,
    Applied,
}
#[derive(Debug)]
pub struct ToolResult {
    pub text: String,
    pub effect: ToolEffect,
    pub exit_code: Option<i32>,
    pub truncated: bool,
    pub timed_out: bool,
    pub change: Option<ChangeDiff>,
}
impl ToolResult {
    fn read_only(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            effect: ToolEffect::ReadOnly,
            exit_code: None,
            truncated: false,
            timed_out: false,
            change: None,
        }
    }
    fn needs_approval(text: impl Into<String>, change: Option<ChangeDiff>) -> Self {
        Self {
            text: text.into(),
            effect: ToolEffect::NeedsApproval,
            exit_code: None,
            truncated: false,
            timed_out: false,
            change,
        }
    }
}

pub struct UserApproval {
    _private: (),
}
impl UserApproval {
    pub fn from_terminal(confirmed: bool) -> Option<Self> {
        confirmed.then_some(Self { _private: () })
    }
}

pub struct ToolDispatcher {
    workspace: Workspace,
    knowledge_db: PathBuf,
}
impl ToolDispatcher {
    pub fn new(workspace: Workspace, knowledge_db: PathBuf) -> Self {
        Self {
            workspace,
            knowledge_db,
        }
    }

    pub fn execute_read_only(&self, request: ToolRequest) -> Result<ToolResult, String> {
        request.validate()?;
        match request {
            ToolRequest::WorkspaceList => {
                let mut names = fs::read_dir(self.workspace.root())
                    .map_err(|e| e.to_string())?
                    .map(|e| {
                        e.map(|v| v.file_name().to_string_lossy().into_owned())
                            .map_err(|e| e.to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                names.sort();
                names.truncate(256);
                Ok(ToolResult::read_only(names.join("\n")))
            }
            ToolRequest::WorkspaceRead { path } => {
                let bytes = self.workspace.read_file(Path::new(&path))?;
                let text =
                    String::from_utf8(bytes).map_err(|_| "workspace file is not UTF-8 text")?;
                Ok(ToolResult::read_only(text))
            }
            ToolRequest::WorkspaceSearch { query } => {
                let index = KnowledgeIndex::open(&self.knowledge_db)?;
                let hits = index.search(&query, 8, 8 * 1024)?;
                Ok(ToolResult::read_only(
                    hits.into_iter()
                        .map(|hit| {
                            format!(
                                "{}:{}-{} [{}]\n{}",
                                hit.path.display(),
                                hit.start_line,
                                hit.end_line,
                                hit.content_hash,
                                hit.text
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                ))
            }
            ToolRequest::SymbolsSearch { query } => {
                let index = KnowledgeIndex::open(&self.knowledge_db)?;
                let hits = index.search_symbols(&query, 30)?;
                Ok(ToolResult::read_only(
                    hits.into_iter()
                        .map(|hit| {
                            format!(
                                "{}:{} {} {}",
                                hit.path.display(),
                                hit.line,
                                hit.kind,
                                hit.signature
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                ))
            }
            ToolRequest::DependenciesSearch { query } => {
                let index = KnowledgeIndex::open(&self.knowledge_db)?;
                let hits = index.search_dependencies(&query, 50)?;
                Ok(ToolResult::read_only(
                    hits.into_iter()
                        .map(|edge| {
                            format!(
                                "{}:{} {} {}",
                                edge.from_path.display(),
                                edge.line,
                                edge.kind,
                                edge.to_name
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                ))
            }
            ToolRequest::GitStatus => run_allowlisted(
                "git",
                &["status", "--short", "--branch"],
                self.workspace.root(),
                Duration::from_secs(8),
                ToolEffect::ReadOnly,
            ),
            ToolRequest::GitDiff => run_allowlisted(
                "git",
                &["diff", "--no-ext-diff", "--no-color", "--stat"],
                self.workspace.root(),
                Duration::from_secs(8),
                ToolEffect::ReadOnly,
            ),
            ToolRequest::NeonAtomEval { source } => {
                let program = parse(&source).map_err(|e| format!("NeonAtom parse error: {e}"))?;
                let value = eval(
                    &program,
                    AtomLimits {
                        fuel: 20_000,
                        max_nesting: 64,
                        max_output_bytes: 8 * 1024,
                        timeout: Duration::from_millis(100),
                    },
                )
                .map_err(|e| format!("NeonAtom evaluation failed: {e}"))?;
                let text = match value {
                    AtomValue::Stream(_) => "<lazy-stream (not materialized)>".to_string(),
                    other => other.to_string(),
                };
                Ok(ToolResult::read_only(text))
            }
            ToolRequest::DuckDuckGoSearch { .. } => {
                Err("web search requires a direct user request; use `neon search <query>`".into())
            }
            ToolRequest::WorkspaceProposeWrite { path, content } => {
                let diff = self
                    .workspace
                    .propose_write(Path::new(&path), content.as_bytes())?;
                let text = diff.preview.clone();
                Ok(ToolResult::needs_approval(text, Some(diff)))
            }
            ToolRequest::CargoTest => Ok(ToolResult::needs_approval(
                "Approval required: cargo test --workspace",
                None,
            )),
            ToolRequest::CargoCheck => Ok(ToolResult::needs_approval(
                "Approval required: cargo check --workspace",
                None,
            )),
        }
    }

    pub fn apply_approved_write(
        &self,
        diff: &ChangeDiff,
        _approval: &UserApproval,
    ) -> Result<ToolResult, String> {
        self.workspace.apply_write(diff, true)?;
        let relative = diff
            .path
            .strip_prefix(self.workspace.root())
            .map_err(|_| "change diff is outside workspace")?;
        let bytes = self.workspace.read_file(relative)?;
        let readback = String::from_utf8(bytes).map_err(|_| "written file is not UTF-8")?;
        if readback != diff.after {
            return Err("write readback does not match approved content".into());
        }
        Ok(ToolResult {
            text: format!(
                "Applied and read back {} bytes at {}",
                readback.len(),
                relative.display()
            ),
            effect: ToolEffect::Applied,
            exit_code: Some(0),
            truncated: false,
            timed_out: false,
            change: None,
        })
    }

    pub fn execute_approved_cargo(
        &self,
        request: ToolRequest,
        _approval: &UserApproval,
    ) -> Result<ToolResult, String> {
        match request {
            ToolRequest::CargoTest => run_allowlisted(
                "cargo",
                &["test", "--workspace"],
                self.workspace.root(),
                Duration::from_secs(120),
                ToolEffect::Applied,
            ),
            ToolRequest::CargoCheck => run_allowlisted(
                "cargo",
                &["check", "--workspace"],
                self.workspace.root(),
                Duration::from_secs(120),
                ToolEffect::Applied,
            ),
            _ => Err("approved Cargo executor accepts only cargo_test or cargo_check".into()),
        }
    }
}

fn parse_args<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| format!("invalid tool arguments: {e}"))
}
fn validate_text(value: &str, name: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{name} must contain 1..={max} bytes"));
    }
    Ok(())
}

fn run_allowlisted(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
    effect: ToolEffect,
) -> Result<ToolResult, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if program == "git" {
        command
            .env("GIT_PAGER", "cat")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE");
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start allowlisted {program}: {e}"))?;
    let stdout = child.stdout.take().ok_or("cannot capture tool stdout")?;
    let stderr = child.stderr.take().ok_or("cannot capture tool stderr")?;
    let out_thread = thread::spawn(move || drain_pipe(stdout, MAX_OUTPUT_BYTES / 2));
    let err_thread = thread::spawn(move || drain_pipe(stderr, MAX_OUTPUT_BYTES / 2));
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("cannot inspect allowlisted process: {e}"))?
        {
            break Some(status);
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let (out, out_cut) = out_thread.join().map_err(|_| "tool stdout reader failed")?;
    let (err, err_cut) = err_thread.join().map_err(|_| "tool stderr reader failed")?;
    let mut text = String::new();
    if !out.is_empty() {
        text.push_str(&String::from_utf8_lossy(&out));
    }
    if !err.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("stderr: ");
        text.push_str(&String::from_utf8_lossy(&err));
    }
    if timed_out {
        text.push_str("\n[process timed out and was killed]");
    }
    let exit_code = status.as_ref().and_then(|s| s.code());
    Ok(ToolResult {
        text,
        effect,
        exit_code,
        truncated: out_cut || err_cut,
        timed_out,
        change: None,
    })
}
fn drain_pipe<R: Read>(mut reader: R, max: usize) -> (Vec<u8>, bool) {
    let mut output = Vec::new();
    let mut buffer = [0u8; 4096];
    let mut truncated = false;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = max.saturating_sub(output.len());
                output.extend_from_slice(&buffer[..n.min(room)]);
                if n > room {
                    truncated = true;
                }
            }
        }
    }
    (output, truncated)
}

#[cfg(test)]
mod tests {
    use super::ToolRequest;
    #[test]
    fn tool_requests_serialize_and_parse_in_protocol_shape() {
        let request = ToolRequest::WorkspaceRead {
            path: "src/lib.rs".into(),
        };
        let json = request.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["tool"], "workspace_read");
        assert_eq!(value["arguments"]["path"], "src/lib.rs");
        assert!(ToolRequest::parse_json(&json).is_ok());
    }
}
