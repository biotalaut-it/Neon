use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_file(contents: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "neonatom-{}-{}.na",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn run_evaluates_program_and_emit_obeys_limit() {
    let file = temp_file("take(range(10, 1000000000000), 3)");
    let run = Command::new(env!("CARGO_BIN_EXE_neonatom"))
        .args(["run", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(String::from_utf8_lossy(&run.stdout).contains("10"));
    let emit = Command::new(env!("CARGO_BIN_EXE_neonatom"))
        .args(["emit", file.to_str().unwrap(), "--limit", "2"])
        .output()
        .unwrap();
    assert!(emit.status.success());
    assert_eq!(String::from_utf8_lossy(&emit.stdout).lines().count(), 2);
    let _ = fs::remove_file(file);
}
