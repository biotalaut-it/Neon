use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Workspace {
    root: PathBuf,
}
#[derive(Debug, Clone)]
pub struct ChangeDiff {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
    pub preview: String,
}
impl Workspace {
    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|e| format!("cannot create workspace: {e}"))?;
        let root =
            fs::canonicalize(root).map_err(|e| format!("cannot canonicalize workspace: {e}"))?;
        Ok(Self { root })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn resolve(&self, relative: &Path, allow_missing: bool) -> Result<PathBuf, String> {
        if relative.as_os_str().is_empty() || relative.is_absolute() {
            return Err("workspace path must be a non-empty relative path".into());
        }
        if relative.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err("workspace path traversal is not allowed".into());
        }
        let mut current = self.root.clone();
        for part in relative.components() {
            if let Component::Normal(piece) = part {
                current.push(piece);
                match fs::symlink_metadata(&current) {
                    Ok(meta) => {
                        if meta.file_type().is_symlink() {
                            return Err("symlinks are not allowed in workspace paths".into());
                        }
                        if current != self.root
                            && !meta.is_dir()
                            && current != self.root.join(relative)
                            && !allow_missing
                        {
                            return Err("workspace parent component is not a directory".into());
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound && allow_missing => {}
                    Err(e) => return Err(format!("cannot inspect workspace path: {e}")),
                }
            }
        }
        if !current.starts_with(&self.root) {
            return Err("resolved path escapes workspace".into());
        }
        Ok(current)
    }
    pub fn read_file(&self, relative: &Path) -> Result<Vec<u8>, String> {
        let path = self.resolve(relative, false)?;
        let meta = fs::metadata(&path).map_err(|e| format!("cannot inspect file: {e}"))?;
        if !meta.is_file() {
            return Err("workspace target is not a regular file".into());
        }
        if meta.len() > 1024 * 1024 {
            return Err("workspace file exceeds 1 MiB".into());
        }
        fs::read(path).map_err(|e| format!("cannot read workspace file: {e}"))
    }
    pub fn propose_write(&self, relative: &Path, content: &[u8]) -> Result<ChangeDiff, String> {
        if content.len() > 1024 * 1024 {
            return Err("workspace write exceeds 1 MiB".into());
        }
        let path = self.resolve(relative, true)?;
        if let Some(parent) = path.parent() {
            if !parent.is_dir() {
                return Err("workspace parent directory does not exist".into());
            }
        }
        let before = match fs::read(&path) {
            Ok(v) => {
                if v.len() > 1024 * 1024 {
                    return Err("existing file exceeds 1 MiB".into());
                }
                String::from_utf8_lossy(&v).into_owned()
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("cannot read current file: {e}")),
        };
        let after = String::from_utf8(content.to_vec())
            .map_err(|_| "only UTF-8 text writes are supported")?;
        let preview = make_preview(&before, &after);
        Ok(ChangeDiff {
            path,
            before,
            after,
            preview,
        })
    }
    pub fn apply_write(&self, diff: &ChangeDiff, approved: bool) -> Result<(), String> {
        if !approved {
            return Err("write requires explicit user approval".into());
        }
        let path = self.resolve(
            diff.path
                .strip_prefix(&self.root)
                .map_err(|_| "diff path is outside workspace")?,
            true,
        )?;
        let parent = path.parent().ok_or("workspace target has no parent")?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let temp = parent.join(format!(".neon-write-{}-{nonce}", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| format!("cannot create temporary workspace file: {e}"))?;
        if let Err(e) = file
            .write_all(diff.after.as_bytes())
            .and_then(|_| file.sync_all())
        {
            let _ = fs::remove_file(&temp);
            return Err(format!("cannot write temporary file: {e}"));
        }
        drop(file);
        if let Err(e) = fs::rename(&temp, &path) {
            let _ = fs::remove_file(&temp);
            return Err(format!("cannot apply workspace change: {e}"));
        }
        Ok(())
    }
}
fn make_preview(before: &str, after: &str) -> String {
    let mut out = String::from("--- current\n+++ proposed\n");
    for line in before.lines().take(80) {
        out.push_str("- ");
        out.push_str(line);
        out.push('\n');
    }
    for line in after.lines().take(80) {
        out.push_str("+ ");
        out.push_str(line);
        out.push('\n');
    }
    out
}
