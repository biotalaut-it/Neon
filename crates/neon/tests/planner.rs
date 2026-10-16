use neon::{
    planner::{NewPlanStep, PlanStatus, StepStatus, TaskStore},
    tools::ToolRequest,
    verification::{EvidenceKind, VerificationEvidence, VerificationStatus},
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
fn db() -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("neon-planner-{}-{id}.db", std::process::id()))
}
fn step(title: &str, tool: ToolRequest, depends_on: Vec<usize>) -> NewPlanStep {
    NewPlanStep {
        title: title.into(),
        instruction: format!("Do {title}"),
        tool,
        depends_on,
        acceptance: format!("Verify {title}"),
    }
}
fn evidence() -> VerificationEvidence {
    VerificationEvidence {
        kind: EvidenceKind::CommandExit,
        status: VerificationStatus::Verified,
        summary: "real process exit 0".into(),
        source: "cargo check".into(),
        exit_code: Some(0),
        recorded_at: 1,
        truncated: false,
    }
}

#[test]
fn planner_rejects_invalid_steps_and_dependencies() {
    let path = db();
    let mut store = TaskStore::open(&path).unwrap();
    assert!(store
        .create_plan("goal", vec![step("only", ToolRequest::GitStatus, vec![])])
        .is_err());
    assert!(store
        .create_plan(
            "goal",
            vec![
                step("a", ToolRequest::GitStatus, vec![1]),
                step("b", ToolRequest::GitStatus, vec![])
            ]
        )
        .is_err());
    assert!(store
        .create_plan("goal", vec![step("a", ToolRequest::GitStatus, vec![]); 13])
        .is_err());
    assert!(store
        .create_plan(
            "goal",
            vec![
                step("a", ToolRequest::GitStatus, vec![]),
                step(
                    "b",
                    ToolRequest::WorkspaceRead {
                        path: "../x".into()
                    },
                    vec![0]
                )
            ]
        )
        .is_err());
    drop(store);
    let _ = fs::remove_file(path);
}

#[test]
fn planner_persists_checkpoints_and_resume_skips_verified_steps() {
    let path = db();
    let mut store = TaskStore::open(&path).unwrap();
    let plan_id = store
        .create_plan(
            "Inspect and verify",
            vec![
                step("inspect", ToolRequest::WorkspaceList, vec![]),
                step("check", ToolRequest::CargoCheck, vec![0]),
            ],
        )
        .unwrap();
    let first = store.claim_next_step(plan_id).unwrap().unwrap();
    assert_eq!(first.ordinal, 0);
    store.finish_step(first.id, &evidence()).unwrap();
    let second = store.claim_next_step(plan_id).unwrap().unwrap();
    assert_eq!(second.ordinal, 1);
    store.fail_step(second.id, "approval was declined").unwrap();
    assert_eq!(store.load_plan(plan_id).unwrap().status, PlanStatus::Failed);
    drop(store);
    let mut store = TaskStore::open(&path).unwrap();
    store.resume(plan_id).unwrap();
    let retry = store.claim_next_step(plan_id).unwrap().unwrap();
    assert_eq!(retry.id, second.id);
    assert_eq!(retry.status, StepStatus::Running);
    store.finish_step(retry.id, &evidence()).unwrap();
    assert_eq!(
        store.load_plan(plan_id).unwrap().status,
        PlanStatus::Completed
    );
    assert!(store.claim_next_step(plan_id).unwrap().is_none());
    drop(store);
    let _ = fs::remove_file(path);
}

#[test]
fn model_plan_draft_is_strict_and_every_tool_is_revalidated() {
    use neon::planner::parse_plan_draft;
    let valid = r#"{"goal":"inspect project","steps":[{"title":"Read README","instruction":"Read the project guide","tool":{"tool":"workspace_read","arguments":{"path":"README.md"}},"depends_on":[],"acceptance":"A real README is returned"},{"title":"Check code","instruction":"Run a compile check","tool":{"tool":"cargo_check","arguments":{}},"depends_on":[0],"acceptance":"Process exits zero"}]}"#;
    let (goal, steps) = parse_plan_draft(valid).unwrap();
    assert_eq!(goal, "inspect project");
    assert_eq!(steps.len(), 2);
    assert!(parse_plan_draft(&valid.replace(
        "\"acceptance\":\"Process exits zero\"}",
        "\"acceptance\":\"Process exits zero\",\"approval\":true}"
    ))
    .is_err());
    assert!(parse_plan_draft(&valid.replace("cargo_check", "run_shell")).is_err());
    assert!(parse_plan_draft(&valid.replace("README.md", "../outside")).is_err());
}

#[test]
fn planner_recovers_interrupted_step_and_rejects_a_second_retry() {
    let path = db();
    let mut store = TaskStore::open(&path).unwrap();
    let id = store
        .create_plan(
            "Recover interrupted task",
            vec![
                step("first", ToolRequest::GitStatus, vec![]),
                step("second", ToolRequest::GitDiff, vec![0]),
            ],
        )
        .unwrap();
    let claimed = store.claim_next_step(id).unwrap().unwrap();
    drop(store);
    let mut store = TaskStore::open(&path).unwrap();
    store.resume(id).unwrap();
    let recovered = store.claim_next_step(id).unwrap().unwrap();
    assert_eq!(recovered.id, claimed.id);
    assert_eq!(recovered.attempts, 2);
    store
        .fail_step(recovered.id, "second attempt failed")
        .unwrap();
    assert!(store.resume(id).is_err());
    drop(store);
    let _ = fs::remove_file(path);
}
