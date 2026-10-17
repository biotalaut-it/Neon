use crate::verification::{VerificationEvidence, VerificationStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairReport {
    pub attempts: usize,
    pub evidence: VerificationEvidence,
}

pub struct TestRepairLoop;
impl TestRepairLoop {
    pub fn run(
        max_attempts: usize,
        mut execute_test: impl FnMut() -> Result<VerificationEvidence, String>,
        mut propose_and_apply_repair: impl FnMut(&VerificationEvidence, usize) -> Result<(), String>,
    ) -> Result<RepairReport, String> {
        if !(1..=3).contains(&max_attempts) {
            return Err("test repair attempt limit must be in 1..=3".into());
        }
        for attempt in 1..=max_attempts {
            let evidence = execute_test()?;
            if evidence.status == VerificationStatus::Verified {
                return Ok(RepairReport {
                    attempts: attempt,
                    evidence,
                });
            }
            if evidence.status != VerificationStatus::Failed {
                return Err(format!(
                    "test attempt {attempt} is not repairable: {}",
                    evidence.summary
                ));
            }
            if attempt == max_attempts {
                return Err(format!(
                    "test repair limit reached after {attempt} attempts: {}",
                    evidence.summary
                ));
            }
            propose_and_apply_repair(&evidence, attempt)?;
        }
        Err("test repair loop exited without a result".into())
    }
}

#[cfg(test)]
mod tests {
    use super::TestRepairLoop;
    use crate::verification::{EvidenceKind, VerificationEvidence, VerificationStatus};
    use std::cell::Cell;

    fn evidence(status: VerificationStatus) -> VerificationEvidence {
        VerificationEvidence {
            kind: EvidenceKind::CommandExit,
            status,
            summary: format!("test result: {status:?}"),
            source: "cargo test".into(),
            exit_code: Some(i32::from(status != VerificationStatus::Verified)),
            recorded_at: 1,
            truncated: false,
        }
    }

    #[test]
    fn retries_failed_test_once_after_approved_repair_hook() {
        let runs = Cell::new(0);
        let repairs = Cell::new(0);
        let report = TestRepairLoop::run(
            2,
            || {
                let run = runs.get() + 1;
                runs.set(run);
                Ok(evidence(if run == 1 {
                    VerificationStatus::Failed
                } else {
                    VerificationStatus::Verified
                }))
            },
            |_, _| {
                repairs.set(repairs.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(runs.get(), 2);
        assert_eq!(repairs.get(), 1);
        assert_eq!(report.attempts, 2);
        assert_eq!(report.evidence.status, VerificationStatus::Verified);
    }

    #[test]
    fn stops_at_limit_and_does_not_repair_unverified_results() {
        let repairs = Cell::new(0);
        assert!(TestRepairLoop::run(
            2,
            || Ok(evidence(VerificationStatus::Failed)),
            |_, _| {
                repairs.set(repairs.get() + 1);
                Ok(())
            },
        )
        .unwrap_err()
        .contains("limit reached"));
        assert_eq!(repairs.get(), 1);
        assert!(TestRepairLoop::run(
            2,
            || Ok(evidence(VerificationStatus::NotRun)),
            |_, _| panic!("not-run evidence must not trigger repair"),
        )
        .unwrap_err()
        .contains("not repairable"));
    }
}
