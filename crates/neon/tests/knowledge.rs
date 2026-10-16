use neon::knowledge::{scan_workspace, IndexLimits, KnowledgeIndex};
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
    let path = std::env::temp_dir().join(format!("neon-{label}-{}-{id}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}
fn limits() -> IndexLimits {
    IndexLimits {
        max_files: 100,
        max_total_bytes: 1024 * 1024,
        max_file_bytes: 1024,
    }
}

#[test]
fn scanner_accepts_utf8_text_and_returns_relative_path_and_digest() {
    let root = temp_dir("scan");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn привет() {}\n").unwrap();
    let files = scan_workspace(&root, &limits()).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].relative_path, Path::new("src/main.rs"));
    assert!(!files[0].content_hash.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scanner_skips_secret_and_binary_files() {
    let root = temp_dir("exclude");
    fs::write(root.join("README.md"), "public documentation").unwrap();
    fs::write(root.join(".env"), "API_TOKEN=must-not-index").unwrap();
    fs::write(root.join("id_rsa.pem"), "PRIVATE KEY").unwrap();
    fs::write(root.join("blob.bin"), [0, 1, 2, 3]).unwrap();
    fs::create_dir(root.join("target")).unwrap();
    fs::write(root.join("target/output.rs"), "fn generated() {} ").unwrap();
    let files = scan_workspace(&root, &limits()).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].relative_path, Path::new("README.md"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scanner_rejects_symlink_and_enforces_file_count_and_byte_caps() {
    let root = temp_dir("limits");
    fs::write(root.join("a.txt"), "1234").unwrap();
    fs::write(root.join("b.txt"), "5678").unwrap();
    let mut one_file = limits();
    one_file.max_files = 1;
    assert!(scan_workspace(&root, &one_file).is_err());
    let mut small_total = limits();
    small_total.max_total_bytes = 4;
    assert!(scan_workspace(&root, &small_total).is_err());
    let mut small_file = limits();
    small_file.max_file_bytes = 3;
    assert!(scan_workspace(&root, &small_file).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let outside = temp_dir("outside");
        fs::write(outside.join("secret.txt"), "outside root").unwrap();
        symlink(outside.join("secret.txt"), root.join("escape.txt")).unwrap();
        assert!(scan_workspace(&root, &limits()).is_err());
        let _ = fs::remove_dir_all(outside);
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fts_search_returns_real_citation_and_reindex_removes_stale_files() {
    let root = temp_dir("rag");
    let db = temp_dir("ragdb").join("neon.db");
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/guide.md"),
        "first line\nrarely searchable zirconium concept\nlast line\n",
    )
    .unwrap();
    let index = KnowledgeIndex::open(&db).unwrap();
    let report = index.reindex(&root, &limits()).unwrap();
    assert_eq!(report.files_indexed, 1);
    let hits = index.search("zirconium", 5, 4096).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, Path::new("docs/guide.md"));
    assert!(hits[0].start_line <= 2 && hits[0].end_line >= 2);
    fs::remove_file(root.join("docs/guide.md")).unwrap();
    index.reindex(&root, &limits()).unwrap();
    assert!(index.search("zirconium", 5, 4096).unwrap().is_empty());
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(db.parent().unwrap());
}

#[test]
fn chunking_and_search_obey_byte_caps() {
    let root = temp_dir("chunks");
    let db = temp_dir("chunksdb").join("neon.db");
    let text = format!("{}\nunique-anchor\n", "x".repeat(4000));
    fs::write(root.join("large.md"), text).unwrap();
    let index = KnowledgeIndex::open(&db).unwrap();
    index.reindex(&root, &limits()).unwrap_err();
    let mut wide = limits();
    wide.max_file_bytes = 8192;
    wide.max_total_bytes = 8192;
    index.reindex(&root, &wide).unwrap();
    let hits = index.search("unique-anchor", 5, 20).unwrap();
    assert!(hits.iter().map(|hit| hit.text.len()).sum::<usize>() <= 20);
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(db.parent().unwrap());
}
