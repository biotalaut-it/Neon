use neon::memory::MemoryStore;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
fn db_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "neon-memory-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
#[test]
fn personal_memory_requires_approval_and_supports_crud() {
    let path = db_path();
    let mut store = MemoryStore::open(&path).unwrap();
    let p = store
        .propose("preference", "I prefer Rust examples")
        .unwrap();
    assert!(store.search("Rust", 10).unwrap().is_empty());
    let id = store.approve(&p.id).unwrap();
    assert_eq!(
        store.search("Rust", 10).unwrap()[0].text,
        "I prefer Rust examples"
    );
    store.update(id, "I prefer concise Rust examples").unwrap();
    assert_eq!(store.search("concise", 10).unwrap()[0].id, id);
    store.forget(id).unwrap();
    assert!(store.list(10).unwrap().is_empty());
    let _ = fs::remove_file(&path);
}
#[test]
fn forget_all_removes_every_approved_memory() {
    let path = db_path();
    let mut store = MemoryStore::open(&path).unwrap();
    let p = store.propose("note", "remember test vector").unwrap();
    store.approve(&p.id).unwrap();
    store.forget_all().unwrap();
    assert!(store.list(10).unwrap().is_empty());
    let _ = fs::remove_file(path);
}

#[test]
fn migration_preserves_existing_memories_and_fts() {
    let path = db_path();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE memories(id INTEGER PRIMARY KEY, category TEXT NOT NULL, text TEXT NOT NULL, created_at INTEGER NOT NULL);
         CREATE VIRTUAL TABLE memories_fts USING fts5(text, category, content='memories', content_rowid='id');
         CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN INSERT INTO memories_fts(rowid,text,category) VALUES(new.id,new.text,new.category); END;
         INSERT INTO memories(category,text,created_at) VALUES('project','legacy project decision',1);
         INSERT INTO memories_fts(rowid,text,category) SELECT id,text,category FROM memories;
         PRAGMA user_version=1;",
    ).unwrap();
    drop(conn);
    let store = MemoryStore::open(&path).unwrap();
    assert_eq!(
        store.search("legacy", 5).unwrap()[0].text,
        "legacy project decision"
    );
    assert!(store.schema_version().unwrap() >= 2);
    drop(store);
    let _ = fs::remove_file(path);
}

#[test]
fn conversation_summary_requires_approval_and_enters_retrieval() {
    let path = db_path();
    let mut store = MemoryStore::open(&path).unwrap();
    let proposal = store
        .propose_summary(
            "neon",
            "Decision: use local SQLite for NEON project memory.",
        )
        .unwrap();
    assert!(store.search_summaries("SQLite", 5).unwrap().is_empty());
    store.approve_summary(&proposal.id).unwrap();
    assert_eq!(
        store.search_summaries("SQLite", 5).unwrap()[0].project,
        "neon"
    );
    let context = store.retrieve_context("SQLite", 8, 4096).unwrap();
    assert!(context
        .iter()
        .any(|entry| entry.kind == "summary" && entry.text.contains("SQLite")));
    drop(store);
    let _ = fs::remove_file(path);
}

#[test]
fn experience_requires_approval_and_keeps_verification_provenance() {
    let path = db_path();
    let mut store = MemoryStore::open(&path).unwrap();
    let proposal = store
        .propose_experience(
            "Fix a Rust borrow error",
            "success",
            "value borrowed across closure",
            "clone the owned value",
            "cargo test passed",
        )
        .unwrap();
    assert!(store.search_experiences("borrow", 5).unwrap().is_empty());
    store.approve_experience(&proposal.id).unwrap();
    let hits = store.search_experiences("borrow", 5).unwrap();
    assert_eq!(hits[0].verification, "cargo test passed");
    assert!(hits[0].historical);
    drop(store);
    let _ = fs::remove_file(path);
}

#[test]
fn built_in_skills_are_real_searchable_idempotent_procedures() {
    let path = db_path();
    let store = MemoryStore::open(&path).unwrap();
    let initial = store.list_skills(20).unwrap();
    assert!(initial.len() >= 4);
    assert!(initial
        .iter()
        .any(|skill| skill.name == "rust-test" && skill.procedure.contains("cargo test")));
    drop(store);
    let store = MemoryStore::open(&path).unwrap();
    assert_eq!(store.list_skills(20).unwrap().len(), initial.len());
    assert!(!store
        .search_skills("debugging root cause", 5)
        .unwrap()
        .is_empty());
    drop(store);
    let _ = fs::remove_file(path);
}

#[test]
fn user_skill_update_refreshes_fts_and_cannot_change_builtins() {
    let path = db_path();
    let mut store = MemoryStore::open(&path).unwrap();
    let proposal = store
        .propose_skill("review-old", "obsolete trigger", "old procedure")
        .unwrap();
    let id = store.approve_skill(&proposal.id).unwrap();
    store
        .update_skill(id, "review-new", "fresh trigger", "new procedure")
        .unwrap();
    assert!(store.search_skills("obsolete", 5).unwrap().is_empty());
    assert_eq!(
        store.search_skills("fresh", 5).unwrap()[0].name,
        "review-new"
    );
    let builtin = store
        .list_skills(20)
        .unwrap()
        .into_iter()
        .find(|skill| skill.builtin)
        .unwrap();
    assert!(store
        .update_skill(builtin.id, "tampered", "x", "y")
        .is_err());
    drop(store);
    let _ = fs::remove_file(path);
}

#[test]
fn context_retrieval_obeys_item_and_byte_caps() {
    let path = db_path();
    let mut store = MemoryStore::open(&path).unwrap();
    for index in 0..5 {
        let proposal = store
            .propose(
                "preference",
                &format!("Rust project preference entry {index}"),
            )
            .unwrap();
        store.approve(&proposal.id).unwrap();
    }
    let context = store
        .retrieve_context("Rust project preference", 2, 256)
        .unwrap();
    assert!(context.len() <= 2);
    assert!(context.iter().map(|entry| entry.text.len()).sum::<usize>() <= 256);
    drop(store);
    let _ = fs::remove_file(path);
}

#[test]
fn fts_stays_consistent_after_update_delete_and_purge() {
    let path = db_path();
    let mut store = MemoryStore::open(&path).unwrap();
    let p = store.propose_summary("p", "obsolete index phrase").unwrap();
    let id = store.approve_summary(&p.id).unwrap();
    store.update_summary(id, "current indexed phrase").unwrap();
    assert!(store.search_summaries("obsolete", 5).unwrap().is_empty());
    assert_eq!(store.search_summaries("current", 5).unwrap().len(), 1);
    store.delete_summary(id).unwrap();
    assert!(store.search_summaries("current", 5).unwrap().is_empty());
    drop(store);
    let _ = fs::remove_file(path);
}
