mod index;
mod scanner;
mod symbols;

pub use index::KnowledgeIndex;
pub use scanner::{scan_workspace, ScannedFile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexLimits {
    pub max_files: usize,
    pub max_total_bytes: usize,
    pub max_file_bytes: usize,
}
impl Default for IndexLimits {
    fn default() -> Self {
        Self {
            max_files: 10_000,
            max_total_bytes: 64 * 1024 * 1024,
            max_file_bytes: 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexReport {
    pub files_indexed: usize,
    pub chunks_indexed: usize,
    pub symbols_indexed: usize,
    pub dependencies_indexed: usize,
    pub bytes_indexed: usize,
    pub removed_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeHit {
    pub path: std::path::PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub content_hash: String,
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolHit {
    pub path: std::path::PathBuf,
    pub name: String,
    pub kind: String,
    pub signature: String,
    pub line: u32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyHit {
    pub from_path: std::path::PathBuf,
    pub to_name: String,
    pub kind: String,
    pub line: u32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyGraphHit {
    pub from_path: std::path::PathBuf,
    pub target: String,
    pub resolved_path: Option<std::path::PathBuf>,
    pub kind: String,
    pub line: u32,
    pub depth: usize,
}
