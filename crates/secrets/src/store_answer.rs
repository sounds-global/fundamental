//! [`StoreAnswer`] — whether the secret store could be asked, or answered no.

use std::fmt;

use fundamental_macros::ErrorOverride;

/// How a request to the secret store failed, which decides whether asking again can help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ErrorOverride)]
pub enum StoreAnswer {
    /// The request did not complete, the store answered `5xx` or `429`, or it missed its deadline.
    #[overrides(retryable, status = 503)]
    Unavailable,
    /// The store answered no: a refusal, a missing secret or field, or no store configured.
    Refused,
}

impl StoreAnswer {
    /// The answer an HTTP `status` amounts to: `5xx` and `429` may clear by themselves.
    #[must_use]
    pub fn of_status(status: u16) -> Self {
        if status >= 500 || status == 429 {
            Self::Unavailable
        } else {
            Self::Refused
        }
    }

    /// The answer `failure` amounts to: whether asking again can help.
    #[must_use]
    pub fn of(failure: &crate::SecretError) -> Self {
        use fundamental_core::Error as _;
        if failure.is_retryable() {
            Self::Unavailable
        } else {
            Self::Refused
        }
    }
}

impl fmt::Display for StoreAnswer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "the secret store is unavailable",
            Self::Refused => "the secret store refused",
        })
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::StoreAnswer;
    use crate::SecretError;

    #[test]
    fn a_server_failure_or_a_rate_limit_is_unavailable_and_a_refusal_is_not() {
        let answered = |status| SecretError::api(status, "secret/data/nats/app", "m");
        for status in [500, 503, 429] {
            assert_eq!(
                StoreAnswer::of(&answered(status)),
                StoreAnswer::Unavailable,
                "{status}"
            );
            assert!(answered(status).is_retryable(), "{status}");
        }
        for status in [400, 403, 404] {
            assert_eq!(
                StoreAnswer::of(&answered(status)),
                StoreAnswer::Refused,
                "{status}"
            );
            assert!(!answered(status).is_retryable(), "{status}");
        }
        assert_eq!(
            StoreAnswer::of(&SecretError::MissingField {
                path: "p".into(),
                key: "k".into()
            }),
            StoreAnswer::Refused
        );
    }
}
