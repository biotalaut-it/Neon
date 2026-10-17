use std::process::Command;

#[test]
fn team_command_shows_all_named_agents_without_model_configuration() {
    let output = Command::new(env!("CARGO_BIN_EXE_neon"))
        .arg("team")
        .output()
        .expect("run neon team");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 roster");
    for line in [
        "Naon — Leader",
        "Susan — Programmer",
        "Fani — Maintenance",
        "Deby — Debugger / Pemecah Masalah",
        "Dyor — Decomposer",
        "Mike — Router",
        "Jordan — Summarizer",
    ] {
        assert!(
            stdout.contains(line),
            "missing roster entry: {line}\n{stdout}"
        );
    }
}

#[test]
fn local_knowledge_and_symbol_commands_work_without_model_or_internet() {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("neon-cli-index-{}-{id}", std::process::id()));
    let workspace = base.join("workspace");
    let data = base.join("data");
    fs::create_dir_all(workspace.join("src")).unwrap();
    fs::write(
        workspace.join("docs.md"),
        "A local reference mentions quasar-protocol.",
    )
    .unwrap();
    fs::write(
        workspace.join("src/lib.rs"),
        "pub mod api;\npub fn build_neon() {}\n",
    )
    .unwrap();
    fs::write(workspace.join("src/api.rs"), "pub fn local_api() {}\n").unwrap();
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_neon"))
            .args(args)
            .env("NEON_WORKSPACE", &workspace)
            .env("NEON_DATA_DIR", &data)
            .output()
            .unwrap()
    };
    let indexed = invoke(&["knowledge", "index"]);
    assert!(
        indexed.status.success(),
        "{}",
        String::from_utf8_lossy(&indexed.stderr)
    );
    let hits = invoke(&["knowledge", "search", "quasar-protocol"]);
    assert!(hits.status.success());
    let stdout = String::from_utf8(hits.stdout).unwrap();
    assert!(stdout.contains("docs.md:1"), "{stdout}");
    let symbols = invoke(&["symbols", "search", "build_neon"]);
    assert!(symbols.status.success());
    assert!(
        String::from_utf8_lossy(&symbols.stdout).contains("src/lib.rs:2 function fn build_neon")
    );
    let graph = invoke(&["knowledge", "graph", "api", "2"]);
    assert!(graph.status.success());
    assert!(
        String::from_utf8_lossy(&graph.stdout).contains("src/api.rs"),
        "{}",
        String::from_utf8_lossy(&graph.stdout)
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
fn confidence_cli_requests_real_evidence_for_a_plan_with_no_test_results() {
    use neon::{
        planner::{NewPlanStep, TaskStore},
        tools::ToolRequest,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base =
        std::env::temp_dir().join(format!("neon-cli-confidence-{}-{id}", std::process::id()));
    let data = base.join("data");
    fs::create_dir_all(&data).unwrap();
    let mut store = TaskStore::open(&data.join("neon.sqlite3")).unwrap();
    let plan_id = store
        .create_plan(
            "Await a real test",
            vec![
                NewPlanStep {
                    title: "Inspect".into(),
                    instruction: "Read status".into(),
                    tool: ToolRequest::GitStatus,
                    depends_on: vec![],
                    acceptance: "Status is returned".into(),
                },
                NewPlanStep {
                    title: "Run test".into(),
                    instruction: "Run Cargo test".into(),
                    tool: ToolRequest::CargoTest,
                    depends_on: vec![0],
                    acceptance: "A real process exits zero".into(),
                },
            ],
        )
        .unwrap();
    drop(store);
    let output = Command::new(env!("CARGO_BIN_EXE_neon"))
        .args(["confidence", &plan_id.to_string()])
        .env("NEON_DATA_DIR", &data)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Confidence not calibrated"), "{stdout}");
    assert!(
        stdout.contains("obtain a real test/check result"),
        "{stdout}"
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
fn knowledge_index_cli_rejects_parent_traversal() {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("neon-cli-path-{}-{id}", std::process::id()));
    let workspace = base.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_neon"))
        .args(["knowledge", "index", "../outside"])
        .env("NEON_WORKSPACE", &workspace)
        .env("NEON_DATA_DIR", base.join("data"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("relative directory inside the workspace")
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
fn persistent_plan_runs_allowlisted_read_only_steps_without_model_configuration() {
    use neon::{
        planner::{NewPlanStep, TaskStore},
        tools::ToolRequest,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("neon-cli-plan-{}-{id}", std::process::id()));
    let workspace = base.join("workspace");
    let data = base.join("data");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("README.md"), "actual local file\n").unwrap();
    let mut store = TaskStore::open(&data.join("neon.sqlite3")).unwrap();
    let plan_id = store
        .create_plan(
            "Read and calculate 2 + 2",
            vec![
                NewPlanStep {
                    title: "Read guide".into(),
                    instruction: "Read file".into(),
                    tool: ToolRequest::WorkspaceRead {
                        path: "README.md".into(),
                    },
                    depends_on: vec![],
                    acceptance: "Nonempty readback".into(),
                },
                NewPlanStep {
                    title: "Calculate".into(),
                    instruction: "Calculate 2 + 2".into(),
                    tool: ToolRequest::NeonAtomEval {
                        source: "2 + 2".into(),
                    },
                    depends_on: vec![0],
                    acceptance: "Exact arithmetic result".into(),
                },
            ],
        )
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_neon"))
        .args(["plan", "run", &plan_id.to_string()])
        .env("NEON_WORKSPACE", &workspace)
        .env("NEON_DATA_DIR", &data)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Evidence: ReadOnlyTool Verified"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Plan ") && stdout.contains("completed; every step has verified evidence"),
        "{stdout}"
    );
    drop(store);
    let _ = fs::remove_dir_all(base);
}

#[test]
fn resources_command_reports_runtime_caps_without_model_configuration() {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("neon-cli-resources-{}-{id}", std::process::id()));
    fs::create_dir_all(&base).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_neon"))
        .arg("resources")
        .env("NEON_DATA_DIR", base.join("data"))
        .env("NEON_CONTEXT", "4096")
        .env("NEON_AGENT_CALLS", "8")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("estimated prompt cap")
            && text.contains("12 items")
            && text.contains("sequential model calls per task: 8")
            && text.contains("live Android RSS/free RAM is not measured"),
        "{text}"
    );
    let _ = fs::remove_dir_all(base);
}
