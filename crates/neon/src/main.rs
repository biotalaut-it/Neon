use neon::{
    agent::{
        model::{LocalModel, RemoteModel},
        router::{AdaptiveReasoning, HybridIntelligenceRouter, ModelRoute, ReasoningMode},
        supervisor::{AgentBudget, AgentResponse, Supervisor},
        AgentRole,
    },
    benchmark::{append_report, latest_report, BenchmarkRunner},
    config::{data_dir, workspace_dir, Config},
    context::ContextManager,
    knowledge::{IndexLimits, KnowledgeIndex},
    lab::Workspace,
    memory::MemoryStore,
    planner::{parse_plan_draft, PlanStatus, TaskStore},
    plugins::{PluginManifest, PluginRegistry},
    search::search_duckduckgo,
    tools::{ToolDispatcher, ToolEffect, ToolRequest, UserApproval},
    vault::SecretVault,
    verification::{
        ConfidenceAction, ConfidenceEngine, EvidenceKind, VerificationEngine, VerificationStatus,
    },
};
use serde::Deserialize;
use std::{
    cell::RefCell,
    collections::VecDeque,
    env, fs,
    io::{self, BufRead, Write},
    path::{Component, Path, PathBuf},
};

fn usage() {
    println!(
        r#"NEON — local AI agent

Usage:
    neon chat | team | resources | benchmark
    neon plugins list|register <manifest.json>|enable <id>|disable <id>|remove <id>|run <id> <alias> <task> <tool-json>
    neon privacy backup <archive.age>|restore <archive.age>|purge
    neon vault list|set <name>|get <name>|delete <name>
  neon ask <question>
    neon ask --remote <complex-question>
  neon search <DuckDuckGo query>
  neon memory list|search <query>|add <category> <text>|edit <id> <text>|forget <id|all>
  neon summary list|search <query>|edit <id> <text>|delete <id>
  neon skills list|search <query>|add <name> <triggers> <procedure>|edit <id> <name> <triggers> <procedure>|delete <id>
  neon experience list|search <query>|add <task> <outcome> <cause> <solution> <verification>|delete <id>
    neon knowledge index [relative-directory]|search <query>|graph <symbol> [depth]
  neon symbols search <name>|dependencies <query>
  neon plan create <goal>|list|show <id>|resume <id>|run <id>
    neon confidence <plan-id>
  neon workspace list|read <path>|write <path>|test|check

Team:
  Naon (Leader), Susan (Programmer), Fani (Maintenance), Deby (Debugger), Dyor (Decomposer), Mike (Router), Jordan (Summarizer)

Environment:
  NEON_LLAMA_SERVER path to llama-server
  NEON_MODEL_PATH path to a local GGUF model
  NEON_WORKSPACE workspace directory
  NEON_DATA_DIR private local data directory
  NEON_CONTEXT context tokens (default 4096)
  NEON_THREADS CPU threads (default <=4)
  NEON_PORT local inference port (default 8089)
  NEON_AGENT_CALLS maximum sequential model calls (default 8)"#
    );
}
fn app_config() -> Result<Config, String> {
    Config::from_env()
}
fn print_team() {
    println!("Tim NEON (satu model lokal, dipakai bergiliran):");
    for role in AgentRole::all() {
        println!("  {} — {}", role.display_name(), role.title());
    }
}
fn print_agent_response(response: &AgentResponse) {
    let team = response
        .roles_used
        .iter()
        .filter(|role| **role != AgentRole::Leader)
        .map(|role| role.display_name())
        .collect::<Vec<_>>()
        .join(" → ");
    println!(
        "[Tim: {team}; panggilan model: {}]\nNaon> {}",
        response.calls_used, response.answer
    );
    for evidence in &response.evidence {
        println!("  Evidence: {evidence}");
    }
}
fn relevant_context(
    memory: &MemoryStore,
    knowledge: &KnowledgeIndex,
    query: &str,
    recent: &VecDeque<(String, String)>,
    budget: &ContextManager,
) -> Result<Vec<String>, String> {
    let entries = memory.retrieve_context(query, 4, budget.max_context_bytes() / 3)?;
    let mut candidates = entries
        .into_iter()
        .map(|e| format!("[{} #{} {}] {}", e.kind, e.source_id, e.category, e.text))
        .collect::<Vec<_>>();
    for hit in knowledge.search(query, 4, budget.max_context_bytes() / 3)? {
        candidates.push(format!(
            "[RAG {}:{}-{} sha256:{}] {}",
            hit.path.display(),
            hit.start_line,
            hit.end_line,
            hit.content_hash,
            hit.text
        ));
    }
    for (user, assistant) in recent.iter().rev().take(budget.max_recent_turns()) {
        candidates.push(format!(
            "Recent user: {user}\nRecent assistant: {assistant}"
        ));
    }
    let selection = budget.select_context(query, candidates)?;
    if selection.dropped_items > 0 {
        eprintln!(
            "Context budget: {} lower-priority item(s), {} bytes omitted.",
            selection.dropped_items, selection.dropped_bytes
        );
    }
    Ok(selection.items)
}
fn search_command(query: &str) -> Result<(), String> {
    for (index, result) in search_duckduckgo(query, 8)?.iter().enumerate() {
        println!(
            "{}. {}\n   {}\n   {}",
            index + 1,
            result.title,
            result.url,
            result.snippet.as_deref().unwrap_or("")
        );
    }
    Ok(())
}
fn selected_index_root(relative: Option<&str>) -> Result<PathBuf, String> {
    let base = workspace_dir()?
        .canonicalize()
        .map_err(|e| format!("cannot resolve workspace: {e}"))?;
    let Some(relative) = relative else {
        return Ok(base);
    };
    let path = Path::new(relative);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("knowledge index must use a relative directory inside the workspace".into());
    }
    let selected = base
        .join(path)
        .canonicalize()
        .map_err(|e| format!("cannot resolve knowledge subdirectory: {e}"))?;
    if !selected.starts_with(&base) || !selected.is_dir() {
        return Err("knowledge index subdirectory escapes workspace or is not a directory".into());
    }
    Ok(selected)
}
fn knowledge_command(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("index") if args.len() <= 2 => {
            let root = selected_index_root(args.get(1).map(String::as_str))?;
            let index = KnowledgeIndex::open(&data_dir()?.join("neon.sqlite3"))?;
            let report = index.reindex(&root, &IndexLimits::default())?;
            println!("Indexed {} files, {} chunks, {} Rust symbols, {} dependencies, {} bytes; removed {} stale files.",report.files_indexed,report.chunks_indexed,report.symbols_indexed,report.dependencies_indexed,report.bytes_indexed,report.removed_files);
            Ok(())
        }
        Some("search") if args.len() > 1 => {
            let index = KnowledgeIndex::open(&data_dir()?.join("neon.sqlite3"))?;
            for hit in index.search(&args[1..].join(" "), 8, 16 * 1024)? {
                println!(
                    "{}:{}-{} [sha256:{}]\n{}\n",
                    hit.path.display(),
                    hit.start_line,
                    hit.end_line,
                    hit.content_hash,
                    hit.text
                );
            }
            Ok(())
        }
        Some("graph") if args.len() >= 2 && args.len() <= 3 => {
            let depth = args
                .get(2)
                .map(|value| value.parse::<usize>().map_err(|_| "invalid graph depth"))
                .transpose()?
                .unwrap_or(2);
            let index = KnowledgeIndex::open(&data_dir()?.join("neon.sqlite3"))?;
            for edge in index.trace_dependency_graph(&args[1], depth, 100)? {
                println!(
                    "{}:{} depth={} {} -> {}",
                    edge.from_path.display(),
                    edge.line,
                    edge.depth,
                    edge.kind,
                    edge.resolved_path
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| format!("{} (unresolved)", edge.target))
                );
            }
            Ok(())
        }
        _ => Err("usage: neon knowledge index [relative-directory]|search <query>|graph <symbol> [depth]".into()),
    }
}
fn symbols_command(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("search") if args.len() > 1 => {
            let index = KnowledgeIndex::open(&data_dir()?.join("neon.sqlite3"))?;
            for hit in index.search_symbols(&args[1..].join(" "), 50)? {
                println!(
                    "{}:{} {} {}",
                    hit.path.display(),
                    hit.line,
                    hit.kind,
                    hit.signature
                );
            }
            Ok(())
        }
        Some("dependencies") if args.len() > 1 => {
            let index = KnowledgeIndex::open(&data_dir()?.join("neon.sqlite3"))?;
            for edge in index.search_dependencies(&args[1..].join(" "), 100)? {
                println!(
                    "{}:{} {} {}",
                    edge.from_path.display(),
                    edge.line,
                    edge.kind,
                    edge.to_name
                );
            }
            Ok(())
        }
        _ => Err("usage: neon symbols search <name>|dependencies <query>".into()),
    }
}
fn with_model<T>(
    f: impl FnOnce(&LocalModel, &Config, &mut MemoryStore) -> Result<T, String>,
) -> Result<T, String> {
    let config = app_config()?;
    let model = LocalModel::start(&config.model).map_err(|e| e.to_string())?;
    let mut memory = MemoryStore::open(&config.data_dir.join("neon.sqlite3"))?;
    let result = f(&model, &config, &mut memory);
    drop(model);
    result
}
fn ask(question: &str) -> Result<(), String> {
    with_model(|model, config, memory| {
        let knowledge = KnowledgeIndex::open(&config.data_dir.join("neon.sqlite3"))?;
        let budget = ContextManager::for_model(config.model.context_tokens, 768)?;
        let mut memories =
            relevant_context(memory, &knowledge, question, &VecDeque::new(), &budget)?;
        if AdaptiveReasoning::choose(question) == ReasoningMode::Test {
            let dispatcher = ToolDispatcher::new(
                Workspace::open(config.workspace.clone())?,
                config.data_dir.join("neon.sqlite3"),
            );
            let request = ToolRequest::CargoTest;
            let proposal = dispatcher.execute_read_only(request.clone())?;
            println!("{}", proposal.text);
            if confirm("Run tests in the networkless Linux sandbox? Build/test code will execute under resource limits.")? {
                let approval =
                    UserApproval::from_terminal(true).ok_or("approval token unavailable")?;
                let result = dispatcher.execute_approved_cargo(request.clone(), &approval)?;
                print!("{}", result.text);
                let evidence = VerificationEngine::verify_tool_result(&request, &result);
                println!("\nEvidence {:?}: {}", evidence.status, evidence.summary);
                memories.push(format!("Current cargo test evidence: {:?}: {}", evidence.status, evidence.summary));
            } else {
                memories.push("Cargo test was not run; user declined approval. No test evidence is available.".into());
            }
        }
        let response = Supervisor::run(
            model,
            question,
            &memories,
            AgentBudget {
                max_calls: config.max_agent_calls,
                max_context_bytes: budget.max_context_bytes(),
                ..AgentBudget::default()
            },
        )
        .map_err(|e| e.to_string())?;
        print_agent_response(&response);
        Ok(())
    })
}
fn ask_remote(question: &str) -> Result<(), String> {
    if HybridIntelligenceRouter::choose(question, true) != ModelRoute::Remote {
        return Err(
            "remote routing is reserved for analysis-class tasks; use the local route".into(),
        );
    }
    if !confirm("Send only this question to the configured external model provider? No local memory, RAG, or tool output will be sent.")? {
        println!("Remote request declined; continuing with local inference.");
        return ask(question);
    }
    let endpoint = env::var("NEON_REMOTE_URL")
        .map_err(|_| "NEON_REMOTE_URL is required for remote routing")?;
    let model_name = env::var("NEON_REMOTE_MODEL")
        .map_err(|_| "NEON_REMOTE_MODEL is required for remote routing")?;
    let passphrase = zeroize::Zeroizing::new(
        rpassword::prompt_password("Secret vault passphrase: ")
            .map_err(|error| format!("cannot read vault passphrase: {error}"))?,
    );
    let vault = SecretVault::open(&data_dir()?.join("neon.sqlite3"))?;
    let api_key = vault
        .get("remote-api-key", &passphrase)?
        .ok_or("remote-api-key is not configured; use `neon vault set remote-api-key`")?;
    let model =
        RemoteModel::new(&endpoint, &model_name, &api_key).map_err(|error| error.to_string())?;
    let completion = model
        .complete(
            &[neon::agent::model::ChatMessage {
                role: "user".into(),
                content: question.to_string(),
            }],
            768,
        )
        .map_err(|error| error.to_string())?;
    println!(
        "[Remote provider; one confirmed request]\nNaon> {}",
        completion.text
    );
    Ok(())
}
fn confirm(prompt: &str) -> Result<bool, String> {
    print!("{prompt} [y/N] ");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}
fn read_clarification(prompt: &str) -> Result<String, String> {
    print!("{prompt} ");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    let answer = answer.trim();
    if answer.is_empty() || answer.len() > 512 {
        return Err("clarification must contain 1..=512 bytes".into());
    }
    Ok(answer.to_string())
}
fn memory_command(args: &[String]) -> Result<(), String> {
    let mut memory = MemoryStore::open(&data_dir()?.join("neon.sqlite3"))?;
    match args.first().map(String::as_str) {
        Some("list") => show_memories(memory.list(50)?),
        Some("search") => show_memories(memory.search(&args[1..].join(" "), 20)?),
        Some("add") if args.len() >= 3 => {
            let proposal = memory.propose(&args[1], &args[2..].join(" "))?;
            println!("Proposed memory [{}]: {}", proposal.category, proposal.text);
            if confirm("Save this memory locally?")? {
                let id = memory.approve(&proposal.id)?;
                println!("Saved memory {id}");
            } else {
                println!("Not saved");
            }
            Ok(())
        }
        Some("edit") if args.len() >= 3 => {
            let id = args[1].parse::<i64>().map_err(|_| "invalid memory id")?;
            let text = args[2..].join(" ");
            println!("Replacement text for memory {id}: {text}");
            if confirm("Apply this local memory edit?")? {
                memory.update(id, &text)?;
                println!("Updated memory {id}");
            }
            Ok(())
        }
        Some("forget") if args.get(1).map(String::as_str) == Some("all") => {
            if confirm("Delete all personal memory records from the local database?")? {
                memory.forget_all()?;
                println!("All personal memories deleted");
            }
            Ok(())
        }
        Some("forget") => {
            let id = args
                .get(1)
                .ok_or("memory forget requires an id or all")?
                .parse::<i64>()
                .map_err(|_| "invalid memory id")?;
            if confirm(&format!(
                "Delete memory record {id} from the local database?"
            ))? {
                memory.forget(id)?;
                println!("Deleted memory {id}");
            }
            Ok(())
        }
        _ => Err(
            "usage: neon memory list | search <query> | add <category> <text> | forget <id|all>"
                .into(),
        ),
    }
}
fn show_memories(rows: Vec<neon::memory::Memory>) -> Result<(), String> {
    for row in rows {
        println!("{} [{}] {}", row.id, row.category, row.text);
    }
    Ok(())
}
fn show_summaries(rows: Vec<neon::memory::SessionSummary>) -> Result<(), String> {
    for row in rows {
        println!("{} [{}] {}", row.id, row.project, row.text);
    }
    Ok(())
}
fn show_skills(rows: Vec<neon::memory::Skill>) -> Result<(), String> {
    for row in rows {
        println!(
            "{} [{}] {}\n  Triggers: {}\n  Procedure: {}",
            row.id,
            if row.builtin { "built-in" } else { "user" },
            row.name,
            row.triggers,
            row.procedure
        );
    }
    Ok(())
}
fn show_experiences(rows: Vec<neon::memory::Experience>) -> Result<(), String> {
    for row in rows {
        println!("{} [historical={}] {} — {}\n  Cause: {}\n  Solution: {}\n  Historical verification: {}",row.id,row.historical,row.task,row.outcome,row.cause,row.solution,row.verification);
    }
    Ok(())
}
fn summary_command(args: &[String]) -> Result<(), String> {
    let mut memory = MemoryStore::open(&data_dir()?.join("neon.sqlite3"))?;
    match args.first().map(String::as_str) {
        Some("list") => show_summaries(memory.list_summaries(50)?),
        Some("search") if args.len() > 1 => {
            show_summaries(memory.search_summaries(&args[1..].join(" "), 20)?)
        }
        Some("edit") if args.len() >= 3 => {
            let id = args[1].parse::<i64>().map_err(|_| "invalid summary id")?;
            let text = args[2..].join(" ");
            println!("Replacement summary text for {id}: {text}");
            if confirm("Apply this local summary edit?")? {
                memory.update_summary(id, &text)?;
                println!("Updated summary {id}");
            }
            Ok(())
        }
        Some("delete") => {
            let id = args
                .get(1)
                .ok_or("summary delete requires an id")?
                .parse::<i64>()
                .map_err(|_| "invalid summary id")?;
            if confirm(&format!("Delete local session summary {id}?"))? {
                memory.delete_summary(id)?;
            }
            Ok(())
        }
        _ => Err("usage: neon summary list|search <query>|edit <id> <text>|delete <id>".into()),
    }
}
fn skills_command(args: &[String]) -> Result<(), String> {
    let mut memory = MemoryStore::open(&data_dir()?.join("neon.sqlite3"))?;
    match args.first().map(String::as_str) {
        Some("list") => show_skills(memory.list_skills(50)?),
        Some("search") if args.len() > 1 => {
            show_skills(memory.search_skills(&args[1..].join(" "), 20)?)
        }
        Some("add") if args.len() >= 4 => {
            let proposal = memory.propose_skill(&args[1], &args[2], &args[3..].join(" "))?;
            println!(
                "Skill proposal:\nName: {}\nTriggers: {}\nProcedure: {}",
                proposal.name, proposal.triggers, proposal.procedure
            );
            if confirm("Save this local skill checklist?")? {
                println!("Saved skill {}", memory.approve_skill(&proposal.id)?);
            }
            Ok(())
        }
        Some("edit") if args.len() >= 5 => {
            let id = args[1].parse::<i64>().map_err(|_| "invalid skill id")?;
            let procedure = args[4..].join(" ");
            println!("Updated skill proposal {id}:\nName: {}\nTriggers: {}\nProcedure: {procedure}",args[2],args[3]);
            if confirm("Apply this local skill edit? Built-in skills cannot be changed.")? {
                memory.update_skill(id, &args[2], &args[3], &procedure)?;
                println!("Updated skill {id}");
            }
            Ok(())
        }
        Some("delete") => {
            let id = args
                .get(1)
                .ok_or("skills delete requires an id")?
                .parse::<i64>()
                .map_err(|_| "invalid skill id")?;
            if confirm(&format!(
                "Delete user skill {id}? Built-in skills cannot be deleted."
            ))? {
                memory.delete_skill(id)?;
            }
            Ok(())
        }
        _ => Err(
            "usage: neon skills list|search <query>|add <name> <triggers> <procedure>|edit <id> <name> <triggers> <procedure>|delete <id>"
                .into(),
        ),
    }
}
fn experience_command(args: &[String]) -> Result<(), String> {
    let mut memory = MemoryStore::open(&data_dir()?.join("neon.sqlite3"))?;
    match args.first().map(String::as_str){Some("list")=>show_experiences(memory.list_experiences(50)?),Some("search") if args.len()>1=>show_experiences(memory.search_experiences(&args[1..].join(" "),20)?),Some("add") if args.len()==2=>{let fields=args[1].split('|').map(str::trim).collect::<Vec<_>>();if fields.len()!=5{return Err("experience add needs five pipe-separated fields: task|outcome|cause|solution|verification".into());}let proposal=memory.propose_experience(fields[0],fields[1],fields[2],fields[3],fields[4])?;println!("Experience proposal:\nTask: {}\nOutcome: {}\nCause: {}\nSolution: {}\nVerification: {}",proposal.task,proposal.outcome,proposal.cause,proposal.solution,proposal.verification);if confirm("Save this experience locally?")?{println!("Saved experience {}",memory.approve_experience(&proposal.id)?);}Ok(())},Some("delete")=>{let id=args.get(1).ok_or("experience delete requires an id")?.parse::<i64>().map_err(|_|"invalid experience id")?;if confirm(&format!("Delete historical experience {id}?"))?{memory.delete_experience(id)?;}Ok(())},_=>Err("usage: neon experience list|search <query>|add 'task|outcome|cause|solution|verification'|delete <id>".into())}
}
fn workspace_command(args: &[String]) -> Result<(), String> {
    if matches!(args.first().map(String::as_str), Some("test" | "check"))
        && args.get(1).is_some_and(|argument| argument == "--repair")
        && args.len() == 2
    {
        return workspace_test_repair(args[0].as_str());
    }
    let ws = Workspace::open(workspace_dir()?)?;
    match args.first().map(String::as_str) {
        Some("list") => {
            for entry in fs::read_dir(ws.root()).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                println!("{}", entry.file_name().to_string_lossy());
            }
            Ok(())
        }
        Some("read") => {
            let rel = args
                .get(1)
                .ok_or("workspace read requires a relative path")?;
            let bytes = ws.read_file(Path::new(rel))?;
            print!("{}", String::from_utf8_lossy(&bytes));
            Ok(())
        }
        Some("write") => {
            let rel = args
                .get(1)
                .ok_or("workspace write requires a relative path")?;
            println!("Enter replacement text. A line containing .end ends input.");
            let mut content = String::new();
            loop {
                let mut line = String::new();
                io::stdin()
                    .read_line(&mut line)
                    .map_err(|e| e.to_string())?;
                if line.trim_end() == ".end" {
                    break;
                }
                content.push_str(&line);
                if content.len() > 1024 * 1024 {
                    return Err("workspace input exceeds 1 MiB".into());
                }
            }
            let dispatcher = ToolDispatcher::new(ws, data_dir()?.join("neon.sqlite3"));
            let proposal = dispatcher.execute_read_only(ToolRequest::WorkspaceProposeWrite {
                path: rel.to_string(),
                content,
            })?;
            let diff = proposal
                .change
                .ok_or("dispatcher returned no write proposal")?;
            println!(
                "{}\n--- exact proposed replacement ---\n{}",
                diff.preview, diff.after
            );
            if confirm("Apply this exact file change?")? {
                let approval =
                    UserApproval::from_terminal(true).ok_or("approval token unavailable")?;
                let result = dispatcher.apply_approved_write(&diff, &approval)?;
                let evidence = VerificationEngine::verify_tool_result(
                    &ToolRequest::WorkspaceProposeWrite {
                        path: rel.to_string(),
                        content: diff.after.clone(),
                    },
                    &result,
                );
                println!("{}; evidence {:?}", result.text, evidence.status);
            } else {
                println!("Change discarded");
            }
            Ok(())
        }
        Some("test") | Some("check") => {
            let request = if args.first().map(String::as_str) == Some("test") {
                ToolRequest::CargoTest
            } else {
                ToolRequest::CargoCheck
            };
            let root = ws.root().display().to_string();
            let dispatcher = ToolDispatcher::new(ws, data_dir()?.join("neon.sqlite3"));
            let pending = dispatcher.execute_read_only(request.clone())?;
            println!("{}\nWorkspace: {root}", pending.text);
            if !confirm(
                "Run this allowlisted Cargo command? Build scripts and project code will execute.",
            )? {
                return Ok(());
            }
            let approval = UserApproval::from_terminal(true).ok_or("approval token unavailable")?;
            let result = dispatcher.execute_approved_cargo(request.clone(), &approval)?;
            print!("{}", result.text);
            let evidence = VerificationEngine::verify_tool_result(&request, &result);
            println!("\nEvidence {:?}: {}", evidence.status, evidence.summary);
            if evidence.status != VerificationStatus::Verified {
                return Err(evidence.summary);
            }
            Ok(())
        }
        _ => Err("usage: neon workspace list | read <path> | write <path> | test [--repair] | check [--repair]".into()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepairPatch {
    path: String,
    content: String,
    rationale: String,
}

fn workspace_test_repair(command: &str) -> Result<(), String> {
    with_model(|model, _, _| {
        let root = workspace_dir()?;
        let workspace = Workspace::open(root)?;
        let dispatcher = ToolDispatcher::new(workspace, data_dir()?.join("neon.sqlite3"));
        let request = if command == "test" {
            ToolRequest::CargoTest
        } else {
            ToolRequest::CargoCheck
        };
        let pending = dispatcher.execute_read_only(request.clone())?;
        println!("{}", pending.text);
        if !confirm("Approve up to two networkless sandbox runs and one repair attempt; file changes need separate approval?")? {
            return Ok(());
        }
        let approval = UserApproval::from_terminal(true).ok_or("approval token unavailable")?;
        let last_output = RefCell::new(String::new());
        let report = neon::repair::TestRepairLoop::run(
            2,
            || {
                let result = dispatcher.execute_approved_cargo(request.clone(), &approval)?;
                *last_output.borrow_mut() = result.text.clone();
                print!("{}", result.text);
                Ok(VerificationEngine::verify_tool_result(&request, &result))
            },
            |evidence, attempt| {
                let diagnostic = last_output
                    .borrow()
                    .chars()
                    .take(12_000)
                    .collect::<String>();
                let prompt = format!(
                    "Repair one Rust project file after this real test result. Return only strict JSON {{\"path\":string,\"content\":string,\"rationale\":string}}. content must be the complete replacement for one existing UTF-8 workspace file, path must be relative. Do not request commands, network, credentials, or additional files.\nAttempt: {attempt}\nEvidence: {}\nCommand output:\n{diagnostic}",
                    evidence.summary
                );
                let completion = model
                    .complete(
                        &[
                            neon::agent::model::ChatMessage {
                                role: "system".into(),
                                content: "You propose a minimal repair only. Never claim tests passed. Output strict JSON without markdown.".into(),
                            },
                            neon::agent::model::ChatMessage {
                                role: "user".into(),
                                content: prompt,
                            },
                        ],
                        2048,
                    )
                    .map_err(|error| error.to_string())?;
                if completion.text.len() > 1024 * 1024 + 4096 {
                    return Err("repair proposal exceeds 1 MiB".into());
                }
                let patch: RepairPatch = serde_json::from_str(completion.text.trim())
                    .map_err(|error| format!("invalid strict repair proposal: {error}"))?;
                if patch.content.len() > 1024 * 1024
                    || patch.rationale.trim().is_empty()
                    || patch.rationale.len() > 2048
                {
                    return Err("repair content or rationale exceeds its limit".into());
                }
                let proposal =
                    dispatcher.execute_read_only(ToolRequest::WorkspaceProposeWrite {
                        path: patch.path,
                        content: patch.content,
                    })?;
                let diff = proposal
                    .change
                    .ok_or("repair dispatcher returned no file proposal")?;
                println!(
                    "Repair rationale: {}\n{}\nExact replacement:\n{}",
                    patch.rationale, diff.preview, diff.after
                );
                if !confirm(
                    "Approve and apply this exact repair? The test will then run once more.",
                )? {
                    return Err("repair declined; workspace was not modified".into());
                }
                let write_approval =
                    UserApproval::from_terminal(true).ok_or("approval token unavailable")?;
                dispatcher.apply_approved_write(&diff, &write_approval)?;
                Ok(())
            },
        )?;
        println!(
            "Repair loop verified after {} test attempt(s): {}",
            report.attempts, report.evidence.summary
        );
        Ok(())
    })
}
fn show_plan(plan: &neon::planner::Plan) {
    println!("Plan {} [{:?}] {}", plan.id, plan.status, plan.goal);
    for step in &plan.steps {
        println!(
            "  {}. [{:?}] {} — {}",
            step.ordinal + 1,
            step.status,
            step.title,
            step.acceptance
        );
        if let Some(evidence) = &step.evidence {
            println!(
                "     evidence: {:?} {:?}: {}",
                evidence.kind, evidence.status, evidence.summary
            );
        }
        if let Some(error) = &step.error {
            println!("     error: {error}");
        }
    }
}
fn plan_command(args: &[String]) -> Result<(), String> {
    let db = data_dir()?.join("neon.sqlite3");
    match args.first().map(String::as_str) {
        Some("create") if args.len() > 1 => with_model(|model, _, _| {
            let goal = args[1..].join(" ");
            let draft = Supervisor::propose_task_plan(model, &goal).map_err(|e| e.to_string())?;
            let (validated_goal, steps) = parse_plan_draft(&draft)?;
            let mut store = TaskStore::open(&db)?;
            let id = store.create_plan(&validated_goal, steps)?;
            show_plan(&store.load_plan(id)?);
            Ok(())
        }),
        Some("list") => {
            let store = TaskStore::open(&db)?;
            for (id, goal, status, updated) in store.list_plans(50)? {
                println!("{id} [{status:?}] updated={updated}: {goal}");
            }
            Ok(())
        }
        Some("show") => {
            let id = args
                .get(1)
                .ok_or("plan show requires an id")?
                .parse::<i64>()
                .map_err(|_| "invalid plan id")?;
            show_plan(&TaskStore::open(&db)?.load_plan(id)?);
            Ok(())
        }
        Some("resume") => {
            let id = args
                .get(1)
                .ok_or("plan resume requires an id")?
                .parse::<i64>()
                .map_err(|_| "invalid plan id")?;
            let mut store = TaskStore::open(&db)?;
            store.resume(id)?;
            show_plan(&store.load_plan(id)?);
            Ok(())
        }
        Some("run") => {
            let id = args
                .get(1)
                .ok_or("plan run requires an id")?
                .parse::<i64>()
                .map_err(|_| "invalid plan id")?;
            run_plan(id, &db)
        }
        _ => Err("usage: neon plan create <goal>|list|show <id>|resume <id>|run <id>".into()),
    }
}

fn confidence_command(args: &[String]) -> Result<(), String> {
    if args.len() != 1 {
        return Err("usage: neon confidence <plan-id>".into());
    }
    let plan_id = args[0].parse::<i64>().map_err(|_| "invalid plan id")?;
    let plan = TaskStore::open(&data_dir()?.join("neon.sqlite3"))?.load_plan(plan_id)?;
    let evidence = plan
        .steps
        .iter()
        .filter_map(|step| step.evidence.as_ref())
        .filter(|item| item.kind == EvidenceKind::CommandExit)
        .cloned()
        .collect::<Vec<_>>();
    let estimate = ConfidenceEngine::estimate(&evidence);
    println!(
        "Plan {} Cargo evidence: {} verified, {} failed, {} not run.",
        plan.id, estimate.verified, estimate.failed, estimate.not_run
    );
    match estimate.lower_bound {
        Some(bound) => println!("95% Wilson lower bound: {:.1}%", f64::from(bound) * 100.0),
        None => println!("Confidence not calibrated: no recorded Cargo test/check evidence."),
    }
    match ConfidenceEngine::next_step(estimate, 0.8) {
        ConfidenceAction::Accept => println!("Action: evidence meets the configured 80% lower-bound threshold."),
        ConfidenceAction::AskForEvidence => println!("Action: obtain a real test/check result before making a confidence claim."),
        ConfidenceAction::TryAlternateMethod => println!("Action: confidence is below threshold; run an independent check or review the failure."),
    }
    Ok(())
}
fn run_plan(plan_id: i64, db: &Path) -> Result<(), String> {
    let root = workspace_dir()?;
    let mut store = TaskStore::open(db)?;
    let initial = store.load_plan(plan_id)?;
    if initial.status == PlanStatus::Failed {
        store.resume(plan_id)?;
    }
    let dispatcher = ToolDispatcher::new(Workspace::open(root)?, db.to_path_buf());
    while let Some(step) = store.claim_next_step(plan_id)? {
        println!(
            "Plan step {}: {}\nInstruction: {}\nAcceptance: {}\nTool request: {}",
            step.ordinal + 1,
            step.title,
            step.instruction,
            step.acceptance,
            step.tool.to_json()?
        );
        let task_context = format!("{}\n{}\n{}", initial.goal, step.title, step.instruction);
        let proposal = match dispatcher.execute_read_only_for_task(step.tool.clone(), &task_context)
        {
            Err(error) if error.starts_with("clarification required:") => {
                let question = error.trim_start_matches("clarification required:").trim();
                let answer = read_clarification(&format!(
                    "Klarifikasi diperlukan: {question} Jawab 'ikut' atau 'tidak ikut'."
                ))?;
                let boundary = match answer.to_ascii_lowercase().as_str() {
                    "ya" | "y" | "iya" | "ikut" | "termasuk" => "batas akhir termasuk",
                    "tidak" | "t" | "tidak ikut" | "tidak termasuk" | "eksklusif" => {
                        "batas akhir tidak termasuk"
                    }
                    _ => return Err("jawab 'ikut' atau 'tidak ikut' untuk batas akhir".into()),
                };
                let clarified_task = format!("{task_context}\nKlarifikasi pengguna: {boundary}");
                dispatcher.execute_read_only_for_task(step.tool.clone(), &clarified_task)?
            }
            result => result?,
        };
        let result = if proposal.effect == ToolEffect::NeedsApproval {
            println!("Tool proposal:\n{}", proposal.text);
            if let Some(diff) = &proposal.change {
                println!("Exact replacement content:\n{}", diff.after);
            }
            if !confirm("Approve this exact action and continue?")? {
                store.fail_step(step.id, "user declined requested tool approval")?;
                return Err("plan paused: approval declined; inspect and resume when ready".into());
            }
            let approval = UserApproval::from_terminal(true).ok_or("approval token unavailable")?;
            if let Some(diff) = proposal.change {
                dispatcher.apply_approved_write(&diff, &approval)?
            } else {
                dispatcher.execute_approved_cargo(step.tool.clone(), &approval)?
            }
        } else {
            proposal
        };
        println!("Tool output:\n{}", result.text);
        let evidence = VerificationEngine::verify_tool_result(&step.tool, &result);
        println!(
            "Evidence: {:?} {:?}: {}",
            evidence.kind, evidence.status, evidence.summary
        );
        if evidence.status != VerificationStatus::Verified {
            store.fail_step(step.id, &evidence.summary)?;
            return Err(format!("plan step {} was not verified", step.ordinal + 1));
        }
        store.finish_step(step.id, &evidence)?;
    }
    let plan = store.load_plan(plan_id)?;
    show_plan(&plan);
    if plan.status == PlanStatus::Completed {
        println!("Plan completed; every step has verified evidence.");
        let mut memory = MemoryStore::open(db)?;
        let solution = plan
            .steps
            .iter()
            .map(|s| format!("{}: {}", s.title, s.instruction))
            .collect::<Vec<_>>()
            .join("\n");
        let verification = plan
            .steps
            .iter()
            .map(|s| {
                s.evidence
                    .as_ref()
                    .map(|e| format!("step {} {:?}: {}", s.ordinal + 1, e.status, e.summary))
                    .unwrap_or_else(|| format!("step {} has no evidence", s.ordinal + 1))
            })
            .collect::<Vec<_>>()
            .join("\n");
        let proposal = memory.propose_experience(
            &plan.goal,
            "Plan completed; all steps verified",
            "No failure observed during this successful run",
            &solution,
            &verification,
        )?;
        println!("Experience Loop proposal:\nTask: {}\nOutcome: {}\nCause: {}\nSolution: {}\nVerification: {}",proposal.task,proposal.outcome,proposal.cause,proposal.solution,proposal.verification);
        if confirm("Save this verified experience locally?")? {
            println!(
                "Saved experience {}",
                memory.approve_experience(&proposal.id)?
            );
        } else {
            println!("Experience not saved.");
        }
    }
    Ok(())
}
fn resources_command() -> Result<(), String> {
    let context_tokens = env::var("NEON_CONTEXT")
        .unwrap_or_else(|_| "4096".into())
        .parse::<u32>()
        .map_err(|_| "invalid NEON_CONTEXT")?;
    if !(256..=32768).contains(&context_tokens) {
        return Err("NEON_CONTEXT must be in 256..=32768".into());
    }
    let default_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .min(4);
    let threads = env::var("NEON_THREADS")
        .ok()
        .map(|value| value.parse::<usize>().map_err(|_| "invalid NEON_THREADS"))
        .transpose()?
        .unwrap_or(default_threads);
    if threads == 0 || threads > 16 {
        return Err("NEON_THREADS must be in 1..=16".into());
    }
    let agent_calls = env::var("NEON_AGENT_CALLS")
        .unwrap_or_else(|_| "8".into())
        .parse::<usize>()
        .map_err(|_| "invalid NEON_AGENT_CALLS")?;
    if !(3..=12).contains(&agent_calls) {
        return Err("NEON_AGENT_CALLS must be in 3..=12".into());
    }
    println!("NEON runtime budgets (estimates, not OS measurements):\n  model context: {context_tokens} tokens; default response reserve: 768 tokens\n  configured llama.cpp CPU threads: {threads}");
    match ContextManager::for_model(context_tokens, 768) {
        Ok(b) => {
            println!("  estimated prompt cap: {} bytes", b.max_prompt_bytes());
            println!(
                "  retrieved context cap: {} bytes / {} items",
                b.max_context_bytes(),
                b.max_context_items()
            );
            println!(
                "  recent volatile history: at most {} turns",
                b.max_recent_turns()
            );
        }
        Err(error) => println!("  context budget unavailable: {error}"),
    }
    println!("  sequential model calls per task: {agent_calls}\n  workspace file cap: 1 MiB; tool output cap: 16 KiB; planner steps: 2..=12; failed-step retry cap: 1");
    let directory = data_dir()?;
    let mut bytes = 0u64;
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "neon.sqlite3" || name.starts_with("neon.sqlite3-") {
            bytes = bytes.saturating_add(entry.metadata().map_err(|e| e.to_string())?.len());
        }
    }
    println!("  local SQLite database/WAL/SHM size: {bytes} bytes\n  live Android RSS/free RAM is not measured; inspect Android system settings for actual memory pressure.");
    Ok(())
}

fn benchmark_command() -> Result<(), String> {
    with_model(|model, config, _| {
        let path = config.data_dir.join("benchmark-history.jsonl");
        let previous = latest_report(&path)?;
        let model_id = config
            .model
            .model_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "local-model".into());
        let report = BenchmarkRunner::run(model, &model_id);
        append_report(&path, &report)?;
        println!(
            "Benchmark {}/{} passed; average latency {:?} ms; total tokens {:?}.",
            report.passed(),
            report.cases.len(),
            report.average_latency_ms(),
            report.total_tokens()
        );
        for case in &report.cases {
            println!(
                "  {}: {} ({} ms, tokens {:?}+{:?}, RSS/HWM {:?}/{:?} KiB){}",
                case.id,
                if case.passed { "PASS" } else { "FAIL" },
                case.latency_ms,
                case.prompt_tokens,
                case.completion_tokens,
                case.resident_kib,
                case.peak_kib,
                case.error
                    .as_deref()
                    .map(|error| format!("; {error}"))
                    .unwrap_or_default()
            );
        }
        if let Some(previous) = previous.filter(|old| old.model_id == report.model_id) {
            let pass_delta = report.passed() as i64 - previous.passed() as i64;
            let latency_delta = report
                .average_latency_ms()
                .zip(previous.average_latency_ms())
                .map(|(current, old)| current as i128 - old as i128);
            let token_delta = report
                .total_tokens()
                .zip(previous.total_tokens())
                .map(|(current, old)| current as i128 - old as i128);
            println!(
                "Previous same-model report: {}/{} passed; average latency {:?} ms; total tokens {:?}.",
                previous.passed(),
                previous.cases.len(),
                previous.average_latency_ms(),
                previous.total_tokens()
            );
            println!(
                "Delta vs previous same-model run: {pass_delta:+} passed; latency {latency_delta:+?} ms; tokens {token_delta:+?}."
            );
        }
        println!("Saved locally to {}", path.display());
        Ok(())
    })
}

fn plugins_command(args: &[String]) -> Result<(), String> {
    let mut registry = PluginRegistry::open(&data_dir()?.join("neon.sqlite3"))?;
    match args.first().map(String::as_str) {
        Some("list") if args.len() == 1 => {
            for plugin in registry.list()? {
                println!(
                    "{} v{} API {} [{}] permissions={:?}",
                    plugin.id,
                    plugin.version,
                    plugin.api_version,
                    if plugin.enabled { "enabled" } else { "disabled" },
                    plugin.permissions
                );
            }
            Ok(())
        }
        Some("register") if args.len() == 2 => {
            let manifest_path = Path::new(&args[1]);
            let metadata = fs::metadata(manifest_path).map_err(|error| error.to_string())?;
            if metadata.len() > 64 * 1024 {
                return Err("plugin manifest exceeds 64 KiB".into());
            }
            let contents = fs::read_to_string(manifest_path)
                .map_err(|error| format!("cannot read plugin manifest: {error}"))?;
            let manifest: PluginManifest = serde_json::from_str(&contents)
                .map_err(|error| format!("invalid strict plugin manifest: {error}"))?;
            neon::plugins::validate_manifest(&manifest)?;
            println!(
                "Register data-only plugin {} v{} API {}; permissions {:?}; tools {:?}. Plugin will remain disabled.",
                manifest.id,
                manifest.version,
                manifest.api_version,
                manifest.permissions,
                manifest.tools.iter().map(|tool| (&tool.name, &tool.target_tool)).collect::<Vec<_>>()
            );
            if confirm("Approve local plugin registry entry? This does not grant system or shell access.")? {
                registry.register(&manifest)?;
                println!("Registered {} disabled.", manifest.id);
            }
            Ok(())
        }
        Some("enable") | Some("disable") if args.len() == 2 => {
            let enabled = args[0] == "enable";
            let plugin = registry
                .list()?
                .into_iter()
                .find(|plugin| plugin.id == args[1])
                .ok_or("plugin id not found")?;
            println!("{} v{} permissions {:?}", plugin.id, plugin.version, plugin.permissions);
            if confirm(if enabled {
                "Enable this plugin's declared allowlisted tool aliases?"
            } else {
                "Disable this plugin?"
            })? {
                registry.set_enabled(&plugin.id, enabled)?;
                println!("Plugin {} is now {}.", plugin.id, if enabled { "enabled" } else { "disabled" });
            }
            Ok(())
        }
        Some("remove") if args.len() == 2 => {
            if confirm(&format!("Remove plugin {} and its aliases?", args[1]))? {
                registry.remove(&args[1])?;
                println!("Plugin removed.");
            }
            Ok(())
        }
        Some("run") if args.len() == 5 => {
            let request = ToolRequest::parse_json(&args[4])?;
            let dispatcher = ToolDispatcher::new(
                Workspace::open(workspace_dir()?)?,
                data_dir()?.join("neon.sqlite3"),
            );
            let result = dispatcher.execute_plugin_tool(
                &args[1],
                &args[2],
                request.clone(),
                &args[3],
            )?;
            if result.effect == ToolEffect::NeedsApproval {
                println!("Plugin action proposal:\n{}", result.text);
                if let Some(change) = &result.change {
                    println!("Exact replacement content:\n{}", change.after);
                }
                if !confirm("Approve this exact plugin action?")? {
                    return Err("plugin action declined; no change was applied".into());
                }
                let approval = UserApproval::from_terminal(true)
                    .ok_or("approval token unavailable")?;
                let applied = match result.change {
                    Some(change) => dispatcher.apply_approved_write(&change, &approval)?,
                    None => dispatcher.execute_approved_cargo(request.clone(), &approval)?,
                };
                println!("{}", applied.text);
                let evidence = VerificationEngine::verify_tool_result(&request, &applied);
                println!("Evidence {:?}: {}", evidence.status, evidence.summary);
            } else {
                println!("{}", result.text);
            }
            Ok(())
        }
        _ => Err("usage: neon plugins list|register <manifest.json>|enable <id>|disable <id>|remove <id>|run <id> <alias> <task> <tool-json>".into()),
    }
}

fn privacy_command(args: &[String]) -> Result<(), String> {
    let directory = data_dir()?;
    let database = directory.join("neon.sqlite3");
    match args.first().map(String::as_str) {
        Some("backup") if args.len() == 2 => {
            let archive = Path::new(&args[1]);
            if !confirm(&format!(
                "Create an encrypted backup at {}?",
                archive.display()
            ))? {
                return Ok(());
            }
            let passphrase = zeroize::Zeroizing::new(
                rpassword::prompt_password("Backup passphrase (min 12 bytes): ")
                    .map_err(|error| format!("cannot read passphrase: {error}"))?,
            );
            neon::privacy::PrivacyManager::backup_encrypted(&database, archive, &passphrase)?;
            println!("Encrypted backup created at {}", archive.display());
            Ok(())
        }
        Some("restore") if args.len() == 2 => {
            let archive = Path::new(&args[1]);
            if !confirm("Restore this encrypted backup over all current NEON SQLite data?")? {
                return Ok(());
            }
            let passphrase = zeroize::Zeroizing::new(
                rpassword::prompt_password("Backup passphrase: ")
                    .map_err(|error| format!("cannot read passphrase: {error}"))?,
            );
            neon::privacy::PrivacyManager::restore_encrypted(
                archive,
                &database,
                &passphrase,
                true,
            )?;
            println!("Backup restored and passed SQLite integrity_check.");
            Ok(())
        }
        Some("purge") if args.len() == 1 => {
            if !confirm("Permanently remove NEON database, WAL/SHM, and benchmark history? This cannot be undone.")? {
                return Ok(());
            }
            neon::privacy::PrivacyManager::purge_local_database(&database, true)?;
            let history = directory.join("benchmark-history.jsonl");
            match fs::remove_file(history) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("cannot remove benchmark history: {error}")),
            }
            println!("Local NEON database and benchmark history removed.");
            Ok(())
        }
        _ => Err("usage: neon privacy backup <archive.age>|restore <archive.age>|purge".into()),
    }
}

fn vault_command(args: &[String]) -> Result<(), String> {
    let mut vault = SecretVault::open(&data_dir()?.join("neon.sqlite3"))?;
    match args.first().map(String::as_str) {
        Some("list") if args.len() == 1 => {
            for name in vault.list()? {
                println!("{name}");
            }
            Ok(())
        }
        Some("set") if args.len() == 2 => {
            let passphrase = zeroize::Zeroizing::new(
                rpassword::prompt_password("Vault passphrase (min 12 bytes): ")
                    .map_err(|error| error.to_string())?,
            );
            let confirmation = zeroize::Zeroizing::new(
                rpassword::prompt_password("Repeat vault passphrase: ")
                    .map_err(|error| error.to_string())?,
            );
            if passphrase.as_str() != confirmation.as_str() {
                return Err("vault passphrases do not match".into());
            }
            let value = zeroize::Zeroizing::new(
                rpassword::prompt_password("Secret value: ").map_err(|error| error.to_string())?,
            );
            if vault.list()?.contains(&args[1])
                && !confirm(&format!("Replace encrypted secret {}?", args[1]))?
            {
                return Ok(());
            }
            vault.set(&args[1], &value, &passphrase)?;
            println!("Secret stored encrypted as {}.", args[1]);
            Ok(())
        }
        Some("get") if args.len() == 2 => {
            let passphrase = zeroize::Zeroizing::new(
                rpassword::prompt_password("Vault passphrase: ")
                    .map_err(|error| error.to_string())?,
            );
            let secret = vault
                .get(&args[1], &passphrase)?
                .ok_or("secret name not found")?;
            println!("{}", secret.as_str());
            Ok(())
        }
        Some("delete") if args.len() == 2 => {
            if confirm(&format!("Delete encrypted secret {}?", args[1]))? {
                vault.delete(&args[1], true)?;
            }
            Ok(())
        }
        _ => Err("usage: neon vault list|set <name>|get <name>|delete <name>".into()),
    }
}

fn chat() -> Result<(), String> {
    with_model(|model, config, memory| {
        println!("NEON siap. Naon adalah Leader; inferensi berjalan lokal di Termux.");
        print_team();
        println!("Gunakan /help untuk perintah.\n");
        let knowledge = KnowledgeIndex::open(&config.data_dir.join("neon.sqlite3"))?;
        let budget = ContextManager::for_model(config.model.context_tokens, 768)?;
        let mut recent = VecDeque::<(String, String)>::new();
        let stdin = io::stdin();
        loop {
            print!("kamu> ");
            io::stdout().flush().map_err(|e| e.to_string())?;
            let mut input = String::new();
            if stdin
                .lock()
                .read_line(&mut input)
                .map_err(|e| e.to_string())?
                == 0
            {
                return Ok(());
            }
            let input = input.trim();
            if input.is_empty() {
                continue;
            }
            if input == "/exit" || input == "/quit" {
                return Ok(());
            }
            if input == "/help" {
                println!("Commands: /help /team /summary /skills /experience /search <query> /memory /forget all /workspace /exit");
                continue;
            }
            if input == "/team" {
                print_team();
                continue;
            }
            if let Some(q) = input.strip_prefix("/search ") {
                search_command(q)?;
                continue;
            }
            if input == "/skills" {
                show_skills(memory.list_skills(30)?)?;
                continue;
            }
            if input == "/experience" {
                show_experiences(memory.list_experiences(20)?)?;
                continue;
            }
            if input == "/summary" {
                if recent.is_empty() {
                    println!("Belum ada giliran untuk diringkas.");
                    continue;
                }
                let summary = Supervisor::summarize_session(
                    model,
                    recent.make_contiguous(),
                    budget.max_context_bytes().min(24 * 1024),
                )
                .map_err(|e| e.to_string())?;
                println!("Jordan — ringkasan untuk ditinjau:\n{summary}");
                if confirm("Simpan ringkasan ini ke memori lokal?")? {
                    let project = config
                        .workspace
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "default".into());
                    let proposal = memory.propose_summary(&project, &summary)?;
                    let id = memory.approve_summary(&proposal.id)?;
                    println!("Ringkasan tersimpan dengan id {id}");
                } else {
                    println!("Ringkasan tidak disimpan.");
                }
                continue;
            }
            if input == "/memory" {
                show_memories(memory.list(30)?)?;
                continue;
            }
            if input == "/forget all" {
                if confirm("Delete all personal memories?")? {
                    memory.forget_all()?;
                }
                continue;
            }
            if input == "/workspace" {
                for entry in fs::read_dir(&config.workspace).map_err(|e| e.to_string())? {
                    println!(
                        "{}",
                        entry
                            .map_err(|e| e.to_string())?
                            .file_name()
                            .to_string_lossy()
                    );
                }
                continue;
            }
            let related = relevant_context(memory, &knowledge, input, &recent, &budget)?;
            let result = Supervisor::run(
                model,
                input,
                &related,
                AgentBudget {
                    max_calls: config.max_agent_calls,
                    max_context_bytes: budget.max_context_bytes(),
                    ..AgentBudget::default()
                },
            )
            .map_err(|e| e.to_string())?;
            print_agent_response(&result);
            let recent_cap = budget.max_context_bytes().min(24 * 1024);
            if input.len().saturating_add(result.answer.len()) <= recent_cap {
                recent.push_back((input.to_string(), result.answer.clone()));
            }
            while recent.len() > budget.max_recent_turns()
                || recent.iter().map(|(u, a)| u.len() + a.len()).sum::<usize>() > recent_cap
                || serde_json::to_string(&recent)
                    .map(|serialized| serialized.len())
                    .unwrap_or(usize::MAX)
                    > 24 * 1024
            {
                recent.pop_front();
            }
        }
    })
}
fn execute() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("chat") => chat(),
        Some("team") => {
            print_team();
            Ok(())
        }
        Some("resources") => resources_command(),
        Some("benchmark") => benchmark_command(),
        Some("plugins") => plugins_command(&args[1..]),
        Some("privacy") => privacy_command(&args[1..]),
        Some("vault") => vault_command(&args[1..]),
        Some("--help") | Some("help") => {
            usage();
            Ok(())
        }
        Some("ask")
            if args.get(1).is_some_and(|argument| argument == "--remote") && args.len() > 2 =>
        {
            ask_remote(&args[2..].join(" "))
        }
        Some("ask") if args.len() > 1 => ask(&args[1..].join(" ")),
        Some("search") if args.len() > 1 => search_command(&args[1..].join(" ")),
        Some("memory") => memory_command(&args[1..]),
        Some("summary") => summary_command(&args[1..]),
        Some("skills") => skills_command(&args[1..]),
        Some("experience") => experience_command(&args[1..]),
        Some("knowledge") => knowledge_command(&args[1..]),
        Some("symbols") => symbols_command(&args[1..]),
        Some("plan") => plan_command(&args[1..]),
        Some("confidence") => confidence_command(&args[1..]),
        Some("workspace") => workspace_command(&args[1..]),
        Some(cmd) => Err(format!(
            "unknown or incomplete command: {cmd}; run neon --help"
        )),
    }
}
fn main() {
    if let Err(error) = execute() {
        eprintln!("neon: {error}");
        std::process::exit(2);
    }
}
