use super::IndexLimits;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub relative_path: PathBuf,
    pub modified_at: i64,
    pub byte_len: usize,
    pub content_hash: String,
    pub text: String,
}

pub fn scan_workspace(root: &Path, limits: &IndexLimits) -> Result<Vec<ScannedFile>, String> {
    if limits.max_files == 0 || limits.max_total_bytes == 0 || limits.max_file_bytes == 0 {
        return Err("index limits must all be greater than zero".into());
    }
    let root =
        fs::canonicalize(root).map_err(|e| format!("cannot canonicalize index root: {e}"))?;
    if !root.is_dir() {
        return Err("index root must be a directory".into());
    }
    let mut result = Vec::new();
    let mut total_bytes = 0usize;
    walk(&root, &root, limits, &mut total_bytes, &mut result)?;
    result.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(result)
}

fn walk(
    root: &Path,
    dir: &Path,
    limits: &IndexLimits,
    total_bytes: &mut usize,
    out: &mut Vec<ScannedFile>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|e| format!("cannot list workspace for indexing: {e}"))?
        .map(|entry| entry.map_err(|e| format!("cannot read workspace entry: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| format!("cannot inspect index candidate: {e}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "symlink is not allowed in indexed workspace: {}",
                path.display()
            ));
        }
        if metadata.is_dir() {
            if excluded_directory(&name) {
                continue;
            }
            walk(root, &path, limits, total_bytes, out)?;
            continue;
        }
        if !metadata.is_file() || excluded_file(&name) {
            continue;
        }
        if metadata.len() as usize > limits.max_file_bytes {
            return Err(format!(
                "index candidate exceeds per-file byte cap: {}",
                path.display()
            ));
        }
        let bytes = fs::read(&path).map_err(|e| format!("cannot read index candidate: {e}"))?;
        if bytes.contains(&0) {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes.clone()) else {
            continue;
        };
        let len = bytes.len();
        let next_total = total_bytes.saturating_add(len);
        if next_total > limits.max_total_bytes {
            return Err("workspace text exceeds total index byte cap".into());
        }
        if out.len() >= limits.max_files {
            return Err("workspace exceeds maximum indexed file count".into());
        }
        let relative_path = path
            .strip_prefix(root)
            .map_err(|_| "index candidate escapes canonical root")?
            .to_path_buf();
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        let content_hash = format!("{:x}", Sha256::digest(&bytes));
        *total_bytes = next_total;
        out.push(ScannedFile {
            relative_path,
            modified_at,
            byte_len: len,
            content_hash,
            text,
        });
    }
    Ok(())
}

fn excluded_directory(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "target"
            | "node_modules"
            | "dist"
            | "build"
            | ".cache"
            | "__pycache__"
            | ".gradle"
            | "vendor"
    )
}
fn excluded_file(name: &str) -> bool {
    name == ".env"
        || name.starts_with(".env.")
        || [".pem", ".key", ".p12", ".pfx", ".der", ".jks", ".keystore"]
            .iter()
            .any(|extension| name.ends_with(extension))
        || ["credential", "secret", "token"]
            .iter()
            .any(|part| name.contains(part))
}
