use crate::tools::{ToolEffect, ToolRequest, ToolResult};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    CommandExit,
    FileDiff,
    IndexCitation,
    ManualApproval,
    ReadOnlyTool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Verified,
    Failed,
    NotRun,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationEvidence {
    pub kind: EvidenceKind,
    pub status: VerificationStatus,
    pub summary: String,
    pub source: String,
    pub exit_code: Option<i32>,
    pub recorded_at: i64,
    pub truncated: bool,
}

pub struct VerificationEngine;
impl VerificationEngine {
    pub fn verify_tool_result(request: &ToolRequest, result: &ToolResult) -> VerificationEvidence {
        let (kind, source) = if result.effect == ToolEffect::NeedsApproval {
            (EvidenceKind::ManualApproval, "user approval required")
        } else {
            match request {
                ToolRequest::WorkspaceProposeWrite { .. } => {
                    (EvidenceKind::FileDiff, "workspace write readback")
                }
                ToolRequest::WorkspaceSearch { .. } => {
                    (EvidenceKind::IndexCitation, "workspace_search")
                }
                ToolRequest::SymbolsSearch { .. } => {
                    (EvidenceKind::IndexCitation, "symbols_search")
                }
                ToolRequest::DependenciesSearch { .. } => {
                    (EvidenceKind::IndexCitation, "dependencies_search")
                }
                ToolRequest::DuckDuckGoSearch { .. } => {
                    (EvidenceKind::IndexCitation, "duckduckgo_search")
                }
                ToolRequest::CargoTest => (EvidenceKind::CommandExit, "cargo test --workspace"),
                ToolRequest::CargoCheck => (EvidenceKind::CommandExit, "cargo check --workspace"),
                ToolRequest::GitStatus => {
                    (EvidenceKind::CommandExit, "git status --short --branch")
                }
                ToolRequest::GitDiff => (
                    EvidenceKind::CommandExit,
                    "git diff --no-ext-diff --no-color --stat",
                ),
                _ => (EvidenceKind::ReadOnlyTool, "local read-only tool result"),
            }
        };
        let status = if result.effect == ToolEffect::NeedsApproval {
            VerificationStatus::NotRun
        } else if result.timed_out || result.exit_code.is_some_and(|code| code != 0) {
            VerificationStatus::Failed
        } else if result.truncated
            || (matches!(kind, EvidenceKind::CommandExit) && result.exit_code.is_none())
        {
            VerificationStatus::NotRun
        } else if result.effect == ToolEffect::Applied && result.exit_code != Some(0) {
            VerificationStatus::Failed
        } else {
            VerificationStatus::Verified
        };
        let mut summary = match status {
            VerificationStatus::Verified => "Tool completed and returned evidence".to_string(),
            VerificationStatus::Failed => "Tool did not complete successfully".to_string(),
            VerificationStatus::NotRun => {
                "Tool result is not sufficient to verify the requested outcome".to_string()
            }
        };
        if let Some(code) = result.exit_code {
            summary.push_str(&format!("; exit_code={code}"));
        }
        if result.timed_out {
            summary.push_str("; timed out");
        }
        if result.truncated {
            summary.push_str("; output truncated, verification withheld");
        }
        if result.effect == ToolEffect::NeedsApproval {
            summary.push_str("; awaiting explicit user approval");
        }
        VerificationEvidence {
            kind,
            status,
            summary,
            source: source.to_string(),
            exit_code: result.exit_code,
            recorded_at: now_epoch(),
            truncated: result.truncated,
        }
    }
}
fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
