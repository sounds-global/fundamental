//! [`WaitCause`] — why a sync session is waiting to reconnect.

use crate::TransportError;
use crate::reconnect_hint::ReconnectHint;

/// The class of failure a [`ConnectionStatus::Waiting`](crate::ConnectionStatus::Waiting)
/// session is waiting out; never the failure's text, which is not safe to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WaitCause {
    /// The connection or its upgrade could not be opened: the hub is unreachable or refused.
    Refused,
    /// A connect or a request got no answer in time.
    TimedOut,
    /// The socket ended.
    Closed,
    /// The hub closed with 1001: it is going away, as a server does on deploy.
    GoingAway,
    /// The hub is overloaded: a 1013 close, or a 503 / 429 upgrade refusal.
    TryAgainLater,
    /// The hub answered authentication or the subscription with an error.
    Rejected,
    /// The device's own offline database failed.
    Local,
}

impl WaitCause {
    /// The class of `error`, unless the server's `hint` says more.
    pub(crate) fn of(error: &TransportError, hint: Option<ReconnectHint>) -> Self {
        Self::hinted(hint).unwrap_or(match error {
            TransportError::Timeout => Self::TimedOut,
            TransportError::Closed => Self::Closed,
            TransportError::Protocol(_) | TransportError::Codec(_) => Self::Rejected,
            TransportError::Connect(_) | TransportError::Other(_) => Self::Refused,
        })
    }

    /// The class a server's hint names, if it names one; otherwise `fallback`.
    pub(crate) fn or_hinted(fallback: Self, hint: Option<ReconnectHint>) -> Self {
        Self::hinted(hint).unwrap_or(fallback)
    }

    fn hinted(hint: Option<ReconnectHint>) -> Option<Self> {
        match hint? {
            ReconnectHint::GoingAway => Some(Self::GoingAway),
            ReconnectHint::TryAgainLater | ReconnectHint::RetryAfter(_) => {
                Some(Self::TryAgainLater)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_transport_error_names_its_class() {
        assert_eq!(
            WaitCause::of(&TransportError::Timeout, None),
            WaitCause::TimedOut
        );
        assert_eq!(
            WaitCause::of(&TransportError::Closed, None),
            WaitCause::Closed
        );
        assert_eq!(
            WaitCause::of(&TransportError::Connect("refused".into()), None),
            WaitCause::Refused
        );
        assert_eq!(
            WaitCause::of(&TransportError::Protocol("denied".into()), None),
            WaitCause::Rejected
        );
    }

    #[test]
    fn a_server_hint_outranks_the_error() {
        let refused = TransportError::Connect("503".into());
        assert_eq!(
            WaitCause::of(
                &refused,
                Some(ReconnectHint::RetryAfter(Duration::from_secs(3)))
            ),
            WaitCause::TryAgainLater
        );
        assert_eq!(
            WaitCause::or_hinted(WaitCause::Closed, Some(ReconnectHint::GoingAway)),
            WaitCause::GoingAway
        );
        assert_eq!(
            WaitCause::or_hinted(WaitCause::Closed, Some(ReconnectHint::TryAgainLater)),
            WaitCause::TryAgainLater
        );
        assert_eq!(
            WaitCause::or_hinted(WaitCause::Closed, None),
            WaitCause::Closed
        );
    }
}
