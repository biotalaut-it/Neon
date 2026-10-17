use neon::knowledge::{IndexLimits, KnowledgeIndex};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
fn temp_dir(label: &str) -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("neon-codebase-{label}-{}-{id}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}
fn limits() -> IndexLimits {
    IndexLimits {
        max_files: 100,
        max_total_bytes: 1024 * 1024,
        max_file_bytes: 1024 * 1024,
    }
}

#[test]
fn rust_index_finds_functions_types_traits_and_use_edges() {
    let root = temp_dir("symbols");
    let db = temp_dir("db").join("neon.db");
    fs::write(root.join("lib.rs"), "use std::fmt;\npub mod api;\npub struct Engine;\npub enum State { Ready }\npub trait Runner {}\npub fn handle() {}\n").unwrap();
    let index = KnowledgeIndex::open(&db).unwrap();
    index.reindex(&root, &limits()).unwrap();
    for name in ["Engine", "State", "Runner", "handle", "api"] {
        assert!(
            index
                .search_symbols(name, 10)
                .unwrap()
                .iter()
                .any(|hit| hit.name == name),
            "missing symbol {name}"
        );
    }
    let edges = index.search_dependencies("fmt", 10).unwrap();
    assert!(edges.iter().any(|edge| edge.to_name.contains("fmt")));
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(db.parent().unwrap());
}

#[test]
fn invalid_rust_remains_full_text_searchable_without_symbols_and_stale_symbols_disappear() {
    let root = temp_dir("invalid");
    let db = temp_dir("db2").join("neon.db");
    fs::write(
        root.join("broken.rs"),
        "fn broken(\nfn obsolete_symbol() {}\n",
    )
    .unwrap();
    let index = KnowledgeIndex::open(&db).unwrap();
    index.reindex(&root, &limits()).unwrap();
    assert!(index
        .search("broken", 10, 2048)
        .unwrap()
        .iter()
        .any(|hit| hit.path == Path::new("broken.rs")));
    assert!(index
        .search_symbols("obsolete_symbol", 10)
        .unwrap()
        .is_empty());
    fs::write(root.join("broken.rs"), "fn current_symbol() {}\n").unwrap();
    index.reindex(&root, &limits()).unwrap();
    assert!(index
        .search_symbols("obsolete_symbol", 10)
        .unwrap()
        .is_empty());
    assert_eq!(index.search_symbols("current_symbol", 10).unwrap().len(), 1);
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(db.parent().unwrap());
}

#[test]
fn dependency_graph_traverses_resolved_rust_modules_with_depth_limits() {
    let root = temp_dir("graph");
    let db = temp_dir("graph-db").join("neon.db");
    fs::write(
        root.join("lib.rs"),
        "mod module_name;\npub fn graph_root_anchor() {}\n",
    )
    .unwrap();
    fs::write(root.join("module_name.rs"), "mod nested;\n").unwrap();
    fs::write(root.join("nested.rs"), "pub fn nested_target() {}\n").unwrap();
    let index = KnowledgeIndex::open(&db).unwrap();
    index.reindex(&root, &limits()).unwrap();

    let graph = index
        .trace_dependency_graph("graph_root_anchor", 2, 20)
        .unwrap();
    assert!(graph.iter().any(|edge| {
        edge.from_path == Path::new("lib.rs")
            && edge.resolved_path.as_deref() == Some(Path::new("module_name.rs"))
            && edge.depth == 1
    }));
    assert!(graph.iter().any(|edge| {
        edge.from_path == Path::new("module_name.rs")
            && edge.resolved_path.as_deref() == Some(Path::new("nested.rs"))
            && edge.depth == 2
    }));
    assert!(index
        .trace_dependency_graph("graph_root_anchor", 9, 20)
        .is_err());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(db.parent().unwrap());
}
