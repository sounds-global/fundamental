//! [`ReconnectHint`] — what a server said about when to come back.

use std::time::Duration;

use tracing::warn;

/// The longest floor any hint may impose; no server can hold a device beyond twice this.
pub(crate) const MAX_HINT_FLOOR: Duration = Duration::from_secs(150);

/// A server's standard signal that the next reconnect should wait.
///
/// A closed, clamped set, so nothing else a server sends can shape the schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReconnectHint {
    /// Close code 1001: the endpoint is going away (RFC 6455 §7.4.1).
    GoingAway,
    /// Close code 1013, or a 503 / 429 upgrade refusal with no usable `Retry-After`.
    TryAgainLater,
    /// A refused upgrade's `Retry-After` delay (RFC 9110 §10.2.3), clamped to [`MAX_HINT_FLOOR`].
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "the browser reports no refused upgrade")
    )]
    RetryAfter(Duration),
}

impl ReconnectHint {
    /// The hint a close frame's `code` carries, if it carries one.
    pub(crate) fn from_close_code(code: u16) -> Option<Self> {
        match code {
            1001 => Some(Self::GoingAway),
            1013 => Some(Self::TryAgainLater),
            _ => None,
        }
    }

    /// The hint a refused 503 or 429 upgrade carries; `Retry-After` in `delay-seconds` form only.
    // Only a native build sees a refused upgrade.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "the browser reports no refused upgrade")
    )]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn from_refusal(status: u16, retry_after: Option<&str>) -> Option<Self> {
        if status != 503 && status != 429 {
            return None;
        }
        let Some(value) = retry_after.map(str::trim) else {
            return Some(Self::TryAgainLater);
        };
        match parse_delay_seconds(value) {
            Some(seconds) => {
                let floor = Duration::from_secs(seconds);
                if floor > MAX_HINT_FLOOR {
                    warn!(
                        retry_after_s = seconds,
                        clamped_to_s = MAX_HINT_FLOOR.as_secs(),
                        "reconnect: clamping a server's Retry-After"
                    );
                    return Some(Self::RetryAfter(MAX_HINT_FLOOR));
                }
                Some(Self::RetryAfter(floor))
            }
            None => {
                warn!(
                    status,
                    retry_after_len = value.len(),
                    "reconnect: ignoring a Retry-After that is not delay-seconds"
                );
                Some(Self::TryAgainLater)
            }
        }
    }

    /// The least time the next reconnect waits.
    pub(crate) fn floor(self) -> Duration {
        match self {
            Self::GoingAway => Duration::from_secs(1),
            Self::TryAgainLater => Duration::from_secs(5),
            Self::RetryAfter(floor) => floor.min(MAX_HINT_FLOOR),
        }
    }
}

/// `delay-seconds = 1*DIGIT` (RFC 9110 §10.2.3); an HTTP-date or an overlong value is `None`.
#[cfg_attr(
    target_arch = "wasm32",
    expect(dead_code, reason = "the browser reports no refused upgrade")
)]
fn parse_delay_seconds(value: &str) -> Option<u64> {
    if value.is_empty() || value.len() > 10 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn going_away_and_try_again_later_are_the_only_close_hints() {
        assert_eq!(
            ReconnectHint::from_close_code(1001),
            Some(ReconnectHint::GoingAway)
        );
        assert_eq!(
            ReconnectHint::from_close_code(1013),
            Some(ReconnectHint::TryAgainLater)
        );
        for code in [1000, 1006, 1008, 1011, 4000] {
            assert_eq!(ReconnectHint::from_close_code(code), None, "code {code}");
        }
    }

    #[test]
    fn floors_are_one_and_five_seconds() {
        assert_eq!(ReconnectHint::GoingAway.floor(), Duration::from_secs(1));
        assert_eq!(ReconnectHint::TryAgainLater.floor(), Duration::from_secs(5));
    }

    #[test]
    fn a_503_retry_after_in_seconds_is_the_floor() {
        let hint = ReconnectHint::from_refusal(503, Some("42"));
        assert_eq!(
            hint,
            Some(ReconnectHint::RetryAfter(Duration::from_secs(42)))
        );
        assert_eq!(
            hint.map(ReconnectHint::floor),
            Some(Duration::from_secs(42))
        );
        assert_eq!(
            ReconnectHint::from_refusal(429, Some(" 7 ")),
            Some(ReconnectHint::RetryAfter(Duration::from_secs(7)))
        );
    }

    #[test]
    fn a_retry_after_beyond_the_ceiling_is_clamped() {
        for value in ["151", "86400", "9999999999"] {
            assert_eq!(
                ReconnectHint::from_refusal(503, Some(value)),
                Some(ReconnectHint::RetryAfter(MAX_HINT_FLOOR)),
                "Retry-After: {value}"
            );
        }
    }

    #[test]
    fn an_http_date_retry_after_is_ignored_but_the_refusal_still_counts() {
        assert_eq!(
            ReconnectHint::from_refusal(503, Some("Wed, 21 Oct 2026 07:28:00 GMT")),
            Some(ReconnectHint::TryAgainLater)
        );
        for value in ["", "-5", "1.5", "99999999999"] {
            assert_eq!(
                ReconnectHint::from_refusal(503, Some(value)),
                Some(ReconnectHint::TryAgainLater),
                "Retry-After: {value:?}"
            );
        }
        assert_eq!(
            ReconnectHint::from_refusal(503, None),
            Some(ReconnectHint::TryAgainLater)
        );
    }

    #[test]
    fn only_503_and_429_refusals_are_hints() {
        for status in [400, 401, 403, 404, 500, 502] {
            assert_eq!(
                ReconnectHint::from_refusal(status, Some("30")),
                None,
                "status {status}"
            );
        }
    }
}
