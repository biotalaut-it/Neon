use neon::{
    tools::{ToolEffect, ToolRequest, ToolResult},
    verification::{EvidenceKind, VerificationEngine, VerificationStatus},
};
fn result(
    exit_code: Option<i32>,
    timed_out: bool,
    truncated: bool,
    effect: ToolEffect,
) -> ToolResult {
    ToolResult {
        text: "runner output".into(),
        effect,
        exit_code,
        truncated,
        timed_out,
        change: None,
    }
}

#[test]
fn zero_exit_marks_command_verified_and_nonzero_timeout_are_failed() {
    let ok = VerificationEngine::verify_tool_result(
        &ToolRequest::CargoTest,
        &result(Some(0), false, false, ToolEffect::Applied),
    );
    assert_eq!(ok.status, VerificationStatus::Verified);
    assert_eq!(ok.kind, EvidenceKind::CommandExit);
    let nonzero = VerificationEngine::verify_tool_result(
        &ToolRequest::CargoTest,
        &result(Some(2), false, false, ToolEffect::Applied),
    );
    assert_eq!(nonzero.status, VerificationStatus::Failed);
    let timeout = VerificationEngine::verify_tool_result(
        &ToolRequest::CargoCheck,
        &result(None, true, false, ToolEffect::Applied),
    );
    assert_eq!(timeout.status, VerificationStatus::Failed);
}

#[test]
fn missing_exit_and_truncated_output_are_not_reported_verified() {
    let no_result = VerificationEngine::verify_tool_result(
        &ToolRequest::CargoCheck,
        &result(None, false, false, ToolEffect::NeedsApproval),
    );
    assert_eq!(no_result.status, VerificationStatus::NotRun);
    let truncated = VerificationEngine::verify_tool_result(
        &ToolRequest::CargoTest,
        &result(Some(0), false, true, ToolEffect::Applied),
    );
    assert_eq!(truncated.status, VerificationStatus::NotRun);
    assert!(truncated.summary.contains("truncated"));
}

#[test]
fn model_prose_is_not_an_input_to_the_verification_engine() {
    let evidence = VerificationEngine::verify_tool_result(
        &ToolRequest::CargoTest,
        &result(Some(1), false, false, ToolEffect::Applied),
    );
    assert_eq!(evidence.status, VerificationStatus::Failed);
    assert!(!evidence.summary.contains("passed"));
}
