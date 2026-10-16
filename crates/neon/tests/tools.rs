use neon::{
    lab::Workspace,
    tools::{ToolDispatcher, ToolEffect, ToolRequest},
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
fn temp_dir(label: &str) -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("neon-tool-{label}-{}-{id}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn tool_parser_rejects_unknown_fields_unknown_tools_and_oversized_payloads() {
    assert!(ToolRequest::parse_json(
        r#"{"tool":"workspace_read","arguments":{"path":"src/lib.rs"}}"#
    )
    .is_ok());
    assert!(ToolRequest::parse_json(
        r#"{"tool":"symbols_search","arguments":{"query":"function_name"}}"#
    )
    .is_ok());
    assert!(ToolRequest::parse_json(
        r#"{"tool":"dependencies_search","arguments":{"query":"crate::module"}}"#
    )
    .is_ok());
    assert!(ToolRequest::parse_json(
        r#"{"tool":"workspace_read","arguments":{"path":"a","extra":true}}"#
    )
    .is_err());
    assert!(
        ToolRequest::parse_json(r#"{"tool":"run_shell","arguments":{"command":"id"}}"#).is_err()
    );
    assert!(ToolRequest::parse_json(&format!(
        "{{\"tool\":\"workspace_search\",\"arguments\":{{\"query\":\"{}\"}}}}",
        "x".repeat(9000)
    ))
    .is_err());
}

#[test]
fn workspace_read_rejects_traversal_and_symlink() {
    let root = temp_dir("paths");
    let outside = temp_dir("outside");
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    fs::write(root.join("safe.txt"), "safe text").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt")).unwrap();
    let dispatcher = ToolDispatcher::new(
        Workspace::open(root.clone()).unwrap(),
        root.join("index.db"),
    );
    assert!(dispatcher
        .execute_read_only(ToolRequest::WorkspaceRead {
            path: "../outside/secret.txt".into()
        })
        .is_err());
    #[cfg(unix)]
    assert!(dispatcher
        .execute_read_only(ToolRequest::WorkspaceRead {
            path: "link.txt".into()
        })
        .is_err());
    let safe = dispatcher
        .execute_read_only(ToolRequest::WorkspaceRead {
            path: "safe.txt".into(),
        })
        .unwrap();
    assert!(safe.text.contains("safe text"));
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn git_tools_are_read_only_allowlisted() {
    let root = temp_dir("git");
    fs::write(root.join("keep.txt"), "unchanged").unwrap();
    let before = fs::read(root.join("keep.txt")).unwrap();
    let dispatcher = ToolDispatcher::new(
        Workspace::open(root.clone()).unwrap(),
        root.join("index.db"),
    );
    let result = dispatcher
        .execute_read_only(ToolRequest::GitStatus)
        .unwrap();
    assert_eq!(result.effect, ToolEffect::ReadOnly);
    assert_eq!(fs::read(root.join("keep.txt")).unwrap(), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn neonatom_eval_uses_bounded_fuel_and_is_deterministic() {
    let root = temp_dir("atom");
    let dispatcher = ToolDispatcher::new(
        Workspace::open(root.clone()).unwrap(),
        root.join("index.db"),
    );
    let result = dispatcher
        .execute_read_only(ToolRequest::NeonAtomEval {
            source: "1 + 2 * 3".into(),
        })
        .unwrap();
    assert_eq!(result.text, "7");
    let expensive = dispatcher.execute_read_only(ToolRequest::NeonAtomEval {
        source: "sum(range(0, 1000000))".into(),
    });
    assert!(expensive.is_err());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn write_proposal_is_inert_until_approval_then_readback_is_verified() {
    use neon::{
        tools::UserApproval,
        verification::{VerificationEngine, VerificationStatus},
    };
    let root = temp_dir("approval");
    let dispatcher = ToolDispatcher::new(
        Workspace::open(root.clone()).unwrap(),
        root.join("index.db"),
    );
    let request = ToolRequest::WorkspaceProposeWrite {
        path: "notes.txt".into(),
        content: "approved text\n".into(),
    };
    let proposal = dispatcher.execute_read_only(request.clone()).unwrap();
    assert_eq!(proposal.effect, ToolEffect::NeedsApproval);
    assert!(!root.join("notes.txt").exists());
    assert!(UserApproval::from_terminal(false).is_none());
    let approval = UserApproval::from_terminal(true).unwrap();
    let result = dispatcher
        .apply_approved_write(proposal.change.as_ref().unwrap(), &approval)
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.join("notes.txt")).unwrap(),
        "approved text\n"
    );
    assert_eq!(
        VerificationEngine::verify_tool_result(&request, &result).status,
        VerificationStatus::Verified
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cargo_request_is_only_a_proposal_until_an_approval_token_is_supplied() {
    let root = temp_dir("cargo-approval");
    let dispatcher = ToolDispatcher::new(
        Workspace::open(root.clone()).unwrap(),
        root.join("index.db"),
    );
    let result = dispatcher
        .execute_read_only(ToolRequest::CargoTest)
        .unwrap();
    assert_eq!(result.effect, ToolEffect::NeedsApproval);
    assert_eq!(result.exit_code, None);
    assert!(!result.text.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn symbol_and_dependency_tools_use_real_local_rust_index() {
    let root = temp_dir("codebase-tools");
    fs::write(
        root.join("lib.rs"),
        "use crate::module_name::Thing;\npub fn neon_tool_symbol() {}\nmod module_name;\n",
    )
    .unwrap();
    fs::write(root.join("module_name.rs"), "pub struct Thing;\n").unwrap();
    let db = root.join("index.db");
    let index = neon::knowledge::KnowledgeIndex::open(&db).unwrap();
    index
        .reindex(&root, &neon::knowledge::IndexLimits::default())
        .unwrap();
    let dispatcher = ToolDispatcher::new(Workspace::open(root.clone()).unwrap(), db);
    let symbols = dispatcher
        .execute_read_only(ToolRequest::SymbolsSearch {
            query: "neon_tool_symbol".into(),
        })
        .unwrap();
    assert!(
        symbols.text.contains("lib.rs") && symbols.text.contains("neon_tool_symbol"),
        "{}",
        symbols.text
    );
    let dependencies = dispatcher
        .execute_read_only(ToolRequest::DependenciesSearch {
            query: "module_name".into(),
        })
        .unwrap();
    assert!(
        dependencies.text.contains("lib.rs") && dependencies.text.contains("module_name"),
        "{}",
        dependencies.text
    );
    let _ = fs::remove_dir_all(root);
}
