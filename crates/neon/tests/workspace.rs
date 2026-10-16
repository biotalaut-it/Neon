use neon::lab::Workspace;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "neon-ws-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
#[test]
fn workspace_rejects_escape_and_requires_approval() {
    let root = temp_root();
    let ws = Workspace::open(root.clone()).unwrap();
    assert!(ws.read_file(std::path::Path::new("../secret")).is_err());
    let proposal = ws
        .propose_write(std::path::Path::new("hello.txt"), b"hello\n")
        .unwrap();
    assert!(proposal.preview.contains("+ hello"));
    assert!(ws.apply_write(&proposal, false).is_err());
    ws.apply_write(&proposal, true).unwrap();
    assert_eq!(
        ws.read_file(std::path::Path::new("hello.txt")).unwrap(),
        b"hello\n"
    );
    let _ = fs::remove_dir_all(root);
}
#[cfg(unix)]
#[test]
fn workspace_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;
    let root = temp_root();
    let outside = temp_root().with_extension("txt");
    fs::write(&outside, b"secret").unwrap();
    let ws = Workspace::open(root.clone()).unwrap();
    symlink(&outside, root.join("link")).unwrap();
    assert!(ws.read_file(std::path::Path::new("link")).is_err());
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_file(outside);
}
