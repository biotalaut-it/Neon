use neon::{
    tools::{ToolEffect, ToolRequest, ToolResult},
    verification::{
        ConfidenceAction, ConfidenceEngine, EvidenceKind, VerificationEngine, VerificationEvidence,
        VerificationStatus,
    },
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

#[test]
fn confidence_uses_only_recorded_verified_or_failed_evidence() {
    let evidence = |status| VerificationEvidence {
        kind: EvidenceKind::CommandExit,
        status,
        summary: "recorded test result".into(),
        source: "cargo test".into(),
        exit_code: None,
        recorded_at: 1,
        truncated: false,
    };
    let empty = ConfidenceEngine::estimate(&[]);
    assert_eq!(empty.lower_bound, None);
    assert_eq!(
        ConfidenceEngine::next_step(empty, 0.8),
        ConfidenceAction::AskForEvidence
    );
    let estimate = ConfidenceEngine::estimate(&[
        evidence(VerificationStatus::Verified),
        evidence(VerificationStatus::Verified),
        evidence(VerificationStatus::Failed),
        evidence(VerificationStatus::NotRun),
    ]);
    assert_eq!(estimate.verified, 2);
    assert_eq!(estimate.failed, 1);
    assert_eq!(estimate.not_run, 1);
    assert!(estimate
        .lower_bound
        .is_some_and(|score| (0.0..0.67).contains(&score)));
    assert_eq!(
        ConfidenceEngine::next_step(estimate, 0.8),
        ConfidenceAction::TryAlternateMethod
    );
    assert_eq!(
        ConfidenceEngine::next_step(estimate, f32::NAN),
        ConfidenceAction::AskForEvidence
    );
}
