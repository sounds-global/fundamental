//! [`Settlement`] — how a delivery is answered, decided from its verdict.

use std::time::Duration;

use fundamental_core::Error as _;
use fundamental_core::retry::Entropy;

use crate::verdict::Verdict;
use crate::{InboxConfig, InboxError, ParkReason};

/// How the inbox answers one delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Settlement {
    /// Done — never deliver it again.
    Ack,
    /// Deliver it again after the delay.
    Nak(Duration),
    /// Record why it was refused, then terminate it. If the record cannot be written, it is
    /// delivered again instead: a delivery is never ended without a durable trace.
    Park {
        reason: ParkReason,
        code: Option<String>,
    },
}

/// The wait after a commit whose answer was lost: the redelivery either finds the row and is
/// acknowledged, or applies the effect once.
const COMMIT_UNKNOWN_DELAY: Duration = Duration::from_secs(1);

impl Settlement {
    /// The one place a verdict becomes an answer, on attempt `delivered` of `config`'s budget.
    pub(crate) fn decide(
        verdict: &Verdict,
        delivered: u64,
        config: &InboxConfig,
        entropy: &mut impl Entropy,
    ) -> Self {
        let exhausted = delivered >= config.max_attempts();
        let park = |reason: ParkReason, code: Option<String>| Self::Park { reason, code };
        let own_code = |error: InboxError| Some(error.error_code().to_owned());
        match verdict {
            Verdict::Processed | Verdict::Duplicate => Self::Ack,
            Verdict::Refused { reason, .. } => park(*reason, reason.code().map(str::to_owned)),
            Verdict::CommitUnknown => Self::Nak(COMMIT_UNKNOWN_DELAY),
            Verdict::Undecodable { .. } if exhausted => park(
                ParkReason::Undecodable,
                ParkReason::Undecodable.code().map(str::to_owned),
            ),
            Verdict::Undecodable { .. } => Self::Nak(config.failed_delay()),
            Verdict::Failed { code, .. } if exhausted => {
                park(ParkReason::AttemptsExhausted, Some(code.clone()))
            }
            Verdict::Failed {
                retryable: true, ..
            } => Self::Nak(config.retry_delay(delivered, entropy)),
            Verdict::Failed {
                retryable: false, ..
            } => Self::Nak(config.failed_delay()),
            Verdict::Panicked if exhausted => park(
                ParkReason::HandlerPanicked,
                ParkReason::HandlerPanicked.code().map(str::to_owned),
            ),
            Verdict::Panicked => Self::Nak(config.failed_delay()),
            Verdict::DeadlineExceeded if exhausted => park(
                ParkReason::AttemptsExhausted,
                own_code(InboxError::DeadlineExceeded {
                    effect: String::new(),
                    deadline_secs: 0,
                }),
            ),
            Verdict::DeadlineExceeded => Self::Nak(config.retry_delay(delivered, entropy)),
        }
    }

    /// The stable name of the answer, for the span and the log.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Ack => "ack",
            Self::Nak(_) => "nak",
            Self::Park { .. } => "park",
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::retry::NoEntropy;

    use super::*;

    /// Without entropy a draw is its window's upper bound, so the schedule is deterministic.
    fn decide(verdict: Verdict, delivered: u64) -> Settlement {
        Settlement::decide(&verdict, delivered, &InboxConfig::default(), &mut NoEntropy)
    }

    fn failed(retryable: bool) -> Verdict {
        Verdict::Failed {
            code: "BIL_002".to_owned(),
            retryable,
        }
    }

    fn parked(reason: ParkReason, code: &str) -> Settlement {
        Settlement::Park {
            reason,
            code: Some(code.to_owned()),
        }
    }

    #[test]
    fn an_applied_or_already_applied_event_is_acknowledged() {
        assert_eq!(decide(Verdict::Processed, 1), Settlement::Ack);
        assert_eq!(decide(Verdict::Duplicate, 9), Settlement::Ack);
    }

    #[test]
    fn a_refusal_is_parked_on_the_first_attempt_with_its_own_code() {
        let verdict = Verdict::Refused {
            reason: ParkReason::EventIdReused,
            detail: String::new(),
        };
        assert_eq!(
            decide(verdict, 1),
            parked(ParkReason::EventIdReused, "SINBOX_007")
        );
    }

    #[test]
    fn a_failure_is_delivered_again_until_the_budget_is_spent() {
        assert_eq!(
            decide(failed(true), 1),
            Settlement::Nak(Duration::from_secs(1))
        );
        assert_eq!(
            decide(failed(true), 3),
            Settlement::Nak(Duration::from_secs(4))
        );
        assert_eq!(
            decide(failed(true), 9),
            Settlement::Nak(Duration::from_secs(60))
        );
        assert_eq!(
            decide(failed(false), 1),
            Settlement::Nak(Duration::from_secs(300))
        );
        for retryable in [true, false] {
            assert_eq!(
                decide(failed(retryable), 10),
                parked(ParkReason::AttemptsExhausted, "BIL_002")
            );
        }
    }

    #[test]
    fn a_retry_is_spread_across_its_window() {
        let config = InboxConfig::default();
        let mut entropy = fundamental_core::retry::OsEntropy;
        for _ in 0..50 {
            let Settlement::Nak(delay) =
                Settlement::decide(&failed(true), 4, &config, &mut entropy)
            else {
                panic!("a retryable failure is delivered again");
            };
            assert!(
                delay <= Duration::from_secs(8),
                "{delay:?} outside the window"
            );
        }
    }

    #[test]
    fn a_budget_lowered_under_a_live_delivery_still_parks_it() {
        assert!(matches!(decide(failed(true), 57), Settlement::Park { .. }));
    }

    #[test]
    fn a_lost_commit_answer_is_asked_again_shortly() {
        assert_eq!(
            decide(Verdict::CommitUnknown, 1),
            Settlement::Nak(COMMIT_UNKNOWN_DELAY)
        );
        assert_eq!(
            decide(Verdict::CommitUnknown, 10),
            Settlement::Nak(COMMIT_UNKNOWN_DELAY)
        );
    }

    #[test]
    fn an_undecodable_payload_waits_for_a_newer_replica_then_is_parked() {
        let undecodable = || Verdict::Undecodable {
            detail: "unknown variant".to_owned(),
        };
        assert_eq!(
            decide(undecodable(), 1),
            Settlement::Nak(Duration::from_secs(300))
        );
        assert_eq!(
            decide(undecodable(), 10),
            parked(ParkReason::Undecodable, "SINBOX_015")
        );
    }

    #[test]
    fn a_panic_is_a_failure_until_the_last_attempt_parks_it() {
        assert_eq!(
            decide(Verdict::Panicked, 1),
            Settlement::Nak(Duration::from_secs(300))
        );
        assert_eq!(
            decide(Verdict::Panicked, 10),
            parked(ParkReason::HandlerPanicked, "SINBOX_009")
        );
    }

    #[test]
    fn a_missed_deadline_is_retried_then_parked() {
        assert_eq!(
            decide(Verdict::DeadlineExceeded, 2),
            Settlement::Nak(Duration::from_secs(2))
        );
        assert_eq!(
            decide(Verdict::DeadlineExceeded, 10),
            parked(ParkReason::AttemptsExhausted, "SINBOX_010")
        );
    }
}
