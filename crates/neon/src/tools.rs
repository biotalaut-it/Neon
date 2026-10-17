use crate::{
    agent::router::TaskIntent,
    knowledge::KnowledgeIndex,
    lab::{ChangeDiff, Workspace},
    plugins::{permission_for_target, PluginRegistry},
};
use neonatom::{
    runtime::eval,
    syntax::{parse, Limits as AtomLimits},
    value::Value as AtomValue,
};
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
    KnowledgeGraph { query: String, max_depth: usize },
    GitStatus,
    GitDiff,
    NeonAtomEval { source: String },
    DuckDuckGoSearch { query: String },
    WorkspaceProposeWrite { path: String, content: String },
    CargoTest,
    CargoCheck,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NeonAtomPolicy {
    Eligible,
    Ineligible,
    NeedsClarification(String),
}

pub fn neonatom_policy(task: &str) -> Result<NeonAtomPolicy, String> {
    let lower = task.to_ascii_lowercase();
    let has_natural_range = lower.contains("sampai") || lower.contains("hingga");
    let has_explicit_boundary = lower.contains("inklusif")
        || lower.contains("eksklusif")
        || lower.contains("termasuk")
        || lower.contains("sebelum")
        || lower.contains("tidak menghitung")
        || lower.contains("ikut dihitung");
    let numeric_count = lower.chars().filter(char::is_ascii_digit).count();
    if has_natural_range && numeric_count > 0 && !has_explicit_boundary {
        return Ok(NeonAtomPolicy::NeedsClarification(
            "Batas rentang belum jelas: apakah angka akhirnya ikut dihitung?".into(),
        ));
    }

    let intent = TaskIntent::classify(task).map_err(|e| e.to_string())?;
    if intent != TaskIntent::Compute {
        return Ok(NeonAtomPolicy::Ineligible);
    }

    let has_numeric_signal = numeric_count > 0;
    let has_operation_signal = lower.contains("range")
        || lower.contains("map")
        || lower.contains("filter")
        || lower.contains("take")
        || lower.contains("sum")
        || lower.contains("kuadrat")
        || lower.contains("jumlah")
        || lower.contains("total")
        || lower.contains("hitung")
        || lower.contains("compute")
        || lower.contains("calculate")
        || lower.contains("+")
        || lower.contains("-")
        || lower.contains("*")
        || lower.contains("/")
        || lower.contains("%")
        || lower.contains("=");
    let has_unsupported_signal = lower.contains("rust")
        || lower.contains("kode")
        || lower.contains("code")
        || lower.contains("debug")
        || lower.contains("bug")
        || lower.contains("file")
        || lower.contains("network")
        || lower.contains("web")
        || lower.contains("shell")
        || lower.contains("git");
    if has_unsupported_signal || !has_numeric_signal || !has_operation_signal {
        return Ok(NeonAtomPolicy::Ineligible);
    }
    Ok(NeonAtomPolicy::Eligible)
}

pub fn should_use_neonatom(task: &str) -> Result<bool, String> {
    match neonatom_policy(task)? {
        NeonAtomPolicy::Eligible => Ok(true),
        NeonAtomPolicy::Ineligible => Ok(false),
        NeonAtomPolicy::NeedsClarification(question) => {
            Err(format!("clarification required: {question}"))
        }
    }
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
struct GraphArgs {
    query: String,
    max_depth: usize,
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
    pub fn tool_name(&self) -> &'static str {
        match self {
            Self::WorkspaceList => "workspace_list",
            Self::WorkspaceRead { .. } => "workspace_read",
            Self::WorkspaceSearch { .. } => "workspace_search",
            Self::SymbolsSearch { .. } => "symbols_search",
            Self::DependenciesSearch { .. } => "dependencies_search",
            Self::KnowledgeGraph { .. } => "knowledge_graph",
            Self::GitStatus => "git_status",
            Self::GitDiff => "git_diff",
            Self::NeonAtomEval { .. } => "neonatom_eval",
            Self::DuckDuckGoSearch { .. } => "duckduckgo_search",
            Self::WorkspaceProposeWrite { .. } => "workspace_propose_write",
            Self::CargoTest => "cargo_test",
            Self::CargoCheck => "cargo_check",
        }
    }

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
            "knowledge_graph" => {
                let args: GraphArgs = parse_args(envelope.arguments)?;
                Self::KnowledgeGraph {
                    query: args.query,
                    max_depth: args.max_depth,
                }
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
            Self::KnowledgeGraph { query, max_depth } => (
                "knowledge_graph",
                serde_json::json!({"query":query,"max_depth":max_depth}),
            ),
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
            Self::KnowledgeGraph { query, max_depth } => {
                validate_text(query, "query", 2048)?;
                if !(1..=8).contains(max_depth) {
                    return Err("knowledge graph depth must be in 1..=8".into());
                }
            }
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
        if matches!(&request, ToolRequest::NeonAtomEval { .. }) {
            return Err("NeonAtom evaluation requires the user's task context".into());
        }
        self.execute_read_only_validated(request)
    }

    pub fn execute_read_only_for_task(
        &self,
        request: ToolRequest,
        task: &str,
    ) -> Result<ToolResult, String> {
        if matches!(&request, ToolRequest::NeonAtomEval { .. }) {
            match neonatom_policy(task)? {
                NeonAtomPolicy::Eligible => {}
                NeonAtomPolicy::Ineligible => {
                    return Err(
                        "NeonAtom is only allowed for clear, supported compute tasks".into(),
                    )
                }
                NeonAtomPolicy::NeedsClarification(question) => {
                    return Err(format!("clarification required: {question}"))
                }
            }
        }
        self.execute_read_only_validated(request)
    }

    pub fn execute_plugin_tool(
        &self,
        plugin_id: &str,
        alias: &str,
        request: ToolRequest,
        task: &str,
    ) -> Result<ToolResult, String> {
        let registry = PluginRegistry::open(&self.knowledge_db)?;
        let Some((target, permissions)) = registry.resolve_tool(plugin_id, alias)? else {
            return Err("plugin is disabled or tool alias was not registered".into());
        };
        if target != request.tool_name() {
            return Err("plugin alias target does not match the typed tool request".into());
        }
        if !permissions.contains(&permission_for_target(&target)) {
            return Err("plugin does not have the permission required by this tool".into());
        }
        self.execute_read_only_for_task(request, task)
    }

    fn execute_read_only_validated(&self, request: ToolRequest) -> Result<ToolResult, String> {
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
            ToolRequest::KnowledgeGraph { query, max_depth } => {
                let index = KnowledgeIndex::open(&self.knowledge_db)?;
                let graph = index.trace_dependency_graph(&query, max_depth, 100)?;
                Ok(ToolResult::read_only(
                    graph
                        .into_iter()
                        .map(|edge| {
                            format!(
                                "{}:{} depth={} {} -> {}",
                                edge.from_path.display(),
                                edge.line,
                                edge.depth,
                                edge.kind,
                                edge.resolved_path
                                    .map(|path| path.display().to_string())
                                    .unwrap_or_else(|| format!("{} (unresolved)", edge.target))
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
            ToolRequest::CargoTest => run_sandboxed_cargo(
                &["test", "--workspace"],
                self.workspace.root(),
                Duration::from_secs(120),
                ToolEffect::Applied,
            ),
            ToolRequest::CargoCheck => run_sandboxed_cargo(
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
    run_command(command, timeout, effect)
}

fn run_sandboxed_cargo(
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
    effect: ToolEffect,
) -> Result<ToolResult, String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (args, cwd, timeout, effect);
        Err("sandboxed Cargo execution requires Linux namespaces and bubblewrap".into())
    }
    #[cfg(target_os = "linux")]
    {
        run_sandboxed_program("cargo", args, cwd, timeout, effect)
    }
}

fn run_sandboxed_program(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
    effect: ToolEffect,
) -> Result<ToolResult, String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (program, args, cwd, timeout, effect);
        return Err("sandboxed execution requires Linux namespaces and bubblewrap".into());
    }
    #[cfg(target_os = "linux")]
    {
        let bwrap = Path::new("/usr/bin/bwrap");
        let prlimit = Path::new("/usr/bin/prlimit");
        if !bwrap.is_file() || !prlimit.is_file() {
            return Err(
                "sandbox unavailable: install bubblewrap and prlimit; command not run".into(),
            );
        }
        let workspace = cwd
            .canonicalize()
            .map_err(|error| format!("cannot resolve sandbox workspace: {error}"))?;
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME is required for sandboxed Cargo")?;
        let cargo_home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cargo"));
        let rustup_home = std::env::var_os("RUSTUP_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".rustup"));
        let registry = cargo_home.join("registry");
        let cargo_bin = cargo_home.join("bin");
        if !registry.is_dir() || !cargo_bin.is_dir() || !rustup_home.is_dir() {
            return Err(
                "sandboxed Cargo requires cached crates and an installed Rust toolchain".into(),
            );
        }
        let mut sandbox_args = vec![
            "--die-with-parent".to_string(),
            "--unshare-user".to_string(),
            "--unshare-pid".to_string(),
            "--unshare-net".to_string(),
        ];
        for path in ["/usr", "/bin", "/lib", "/lib64"] {
            if Path::new(path).exists() {
                sandbox_args.extend(["--ro-bind".into(), path.into(), path.into()]);
            }
        }
        sandbox_args.extend([
            "--dev".into(),
            "/dev".into(),
            "--proc".into(),
            "/proc".into(),
            "--size".into(),
            "536870912".into(),
            "--tmpfs".into(),
            "/tmp".into(),
            "--dir".into(),
            "/tmp/home".into(),
            "--dir".into(),
            "/tmp/cargo".into(),
            "--dir".into(),
            "/tmp/cargo/registry".into(),
            "--dir".into(),
            "/tmp/target".into(),
            "--ro-bind".into(),
            registry.to_string_lossy().into_owned(),
            "/tmp/cargo/registry".into(),
            "--dir".into(),
            "/opt".into(),
            "--dir".into(),
            "/opt/rustup".into(),
            "--ro-bind".into(),
            rustup_home.to_string_lossy().into_owned(),
            "/opt/rustup".into(),
            "--dir".into(),
            "/tools".into(),
            "--ro-bind".into(),
            cargo_bin.to_string_lossy().into_owned(),
            "/tools".into(),
            "--dir".into(),
            "/workspace".into(),
            "--ro-bind".into(),
            workspace.to_string_lossy().into_owned(),
            "/workspace".into(),
            "--chdir".into(),
            "/workspace".into(),
            "--setenv".into(),
            "HOME".into(),
            "/tmp/home".into(),
            "--setenv".into(),
            "CARGO_HOME".into(),
            "/tmp/cargo".into(),
            "--setenv".into(),
            "RUSTUP_HOME".into(),
            "/opt/rustup".into(),
            "--setenv".into(),
            "CARGO_TARGET_DIR".into(),
            "/tmp/target".into(),
            "--setenv".into(),
            "CARGO_NET_OFFLINE".into(),
            "true".into(),
            "--setenv".into(),
            "CARGO_BUILD_JOBS".into(),
            "2".into(),
            "--setenv".into(),
            "PATH".into(),
            "/tools:/usr/bin:/bin".into(),
            "--".into(),
            match program {
                "cargo" => "/tools/cargo".to_string(),
                _ => return Err("sandbox executor rejected a non-Cargo command".into()),
            },
        ]);
        sandbox_args.extend(args.iter().map(|argument| (*argument).to_string()));
        sandbox_args.push("--locked".into());
        let mut command = Command::new(prlimit);
        command
            .args([
                "--as=4294967296",
                "--cpu=180",
                "--fsize=1073741824",
                "--nofile=256",
                "--nproc=512",
                "--",
            ])
            .arg(bwrap)
            .args(sandbox_args)
            .current_dir(&workspace)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        run_command(command, timeout, effect)
    }
}

fn run_command(
    mut command: Command,
    timeout: Duration,
    effect: ToolEffect,
) -> Result<ToolResult, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start bounded tool process: {e}"))?;
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
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
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

    #[cfg(unix)]
    #[test]
    fn timed_out_process_kills_descendants_in_its_process_group() {
        use super::{run_command, ToolEffect};
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let started = Instant::now();
        let result = run_command(
            {
                let mut command = Command::new("sh");
                command.args(["-c", "sleep 5 & wait"]);
                command.stdout(Stdio::piped()).stderr(Stdio::piped());
                command
            },
            Duration::from_millis(100),
            ToolEffect::Applied,
        )
        .unwrap();
        assert!(result.timed_out);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
