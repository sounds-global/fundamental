//! [`CloseReason`]: why a connection ended, with stable codes.

use std::time::Duration;

use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

/// Why a connection ended; each reason's close code and metric label are fixed by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CloseReason {
    /// The upgrade did not complete within `handshake_timeout`; the TCP connection is dropped.
    HandshakeTimeout,
    /// The upgrade was not a valid WebSocket handshake; the TCP connection is dropped.
    HandshakeRejected,
    /// No principal was bound within the authentication deadline — `4408`.
    AuthDeadline,
    /// The anonymous connection gave its slot to a newcomer past `displace_after` — `1013`.
    Displaced,
    /// The anonymous share was full and none could be displaced — `1013`. A close frame, not an
    /// upgrade refusal, because a browser cannot read a failed upgrade's status.
    OverShare,
    /// A principal was verified but no authenticated slot was free — `1013`.
    Saturated,
    /// The principal already held its most connections — `1008`.
    PrincipalLimit,
    /// The peer did not drain an outbound frame within `send_timeout` — `1008`, best effort.
    SlowPeer,
    /// An anonymous connection had more frames refused than it may — `1008`.
    Refused,
    /// Reading or writing the socket failed — `1002`, best effort.
    TransportError,
    /// The client closed the connection — `1000`.
    ClientClosed,
    /// The server is stopping and the connection has drained — `1001`.
    GoingAway,
    /// The session was aborted at the end of `drain_timeout`.
    DrainTimeout,
}

impl CloseReason {
    /// Every reason, in the order [`index`](Self::index) numbers them.
    pub const ALL: [Self; 13] = [
        Self::HandshakeTimeout,
        Self::HandshakeRejected,
        Self::AuthDeadline,
        Self::Displaced,
        Self::OverShare,
        Self::Saturated,
        Self::PrincipalLimit,
        Self::SlowPeer,
        Self::Refused,
        Self::TransportError,
        Self::ClientClosed,
        Self::GoingAway,
        Self::DrainTimeout,
    ];

    /// The WebSocket close code sent with this reason, or `None` when no frame is sent.
    #[must_use]
    pub const fn code(self) -> Option<u16> {
        match self {
            Self::HandshakeTimeout | Self::HandshakeRejected | Self::DrainTimeout => None,
            Self::AuthDeadline => Some(4408),
            Self::Displaced | Self::OverShare | Self::Saturated => Some(1013),
            Self::SlowPeer | Self::Refused | Self::PrincipalLimit => Some(1008),
            Self::TransportError => Some(1002),
            Self::ClientClosed => Some(1000),
            Self::GoingAway => Some(1001),
        }
    }

    /// The reason's metric label and log value.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::HandshakeTimeout => "handshake_timeout",
            Self::HandshakeRejected => "handshake_rejected",
            Self::AuthDeadline => "auth_deadline",
            Self::Displaced => "displaced",
            Self::OverShare => "over_share",
            Self::Saturated => "saturated",
            Self::PrincipalLimit => "principal_limit",
            Self::SlowPeer => "slow_peer",
            Self::Refused => "refused",
            Self::TransportError => "transport_error",
            Self::ClientClosed => "client_closed",
            Self::GoingAway => "going_away",
            Self::DrainTimeout => "drain_timeout",
        }
    }

    /// The reason's position in [`ALL`](Self::ALL), for per-reason counters.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::HandshakeTimeout => 0,
            Self::HandshakeRejected => 1,
            Self::AuthDeadline => 2,
            Self::Displaced => 3,
            Self::OverShare => 4,
            Self::Saturated => 5,
            Self::PrincipalLimit => 6,
            Self::SlowPeer => 7,
            Self::Refused => 8,
            Self::TransportError => 9,
            Self::ClientClosed => 10,
            Self::GoingAway => 11,
            Self::DrainTimeout => 12,
        }
    }

    /// Whether the server sends a close frame for this reason.
    pub(crate) const fn sends_close_frame(self) -> bool {
        !matches!(
            self,
            Self::HandshakeTimeout
                | Self::HandshakeRejected
                | Self::ClientClosed
                | Self::DrainTimeout
        )
    }

    /// The close frame for this reason, its text `<label>` or `<label>;retry_after_ms=<n>`, well under 123 bytes.
    pub(crate) fn close_frame(self, retry_after: Option<Duration>) -> Option<CloseFrame> {
        self.code().map(|code| CloseFrame {
            code: CloseCode::from(code),
            reason: match retry_after {
                Some(after) => {
                    format!("{};retry_after_ms={}", self.label(), after.as_millis()).into()
                }
                None => self.label().into(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_numbers_all_in_order() {
        for (position, reason) in CloseReason::ALL.iter().enumerate() {
            assert_eq!(reason.index(), position, "{reason:?}");
        }
    }

    #[test]
    fn the_documented_codes_are_stable() {
        assert_eq!(CloseReason::AuthDeadline.code(), Some(4408));
        assert_eq!(CloseReason::Displaced.code(), Some(1013));
        assert_eq!(CloseReason::OverShare.code(), Some(1013));
        assert_eq!(CloseReason::Saturated.code(), Some(1013));
        assert_eq!(CloseReason::SlowPeer.code(), Some(1008));
        assert_eq!(CloseReason::Refused.code(), Some(1008));
        assert_eq!(CloseReason::TransportError.code(), Some(1002));
        assert_eq!(CloseReason::ClientClosed.code(), Some(1000));
        assert_eq!(CloseReason::GoingAway.code(), Some(1001));
        assert_eq!(CloseReason::DrainTimeout.code(), None);
        assert_eq!(CloseReason::HandshakeTimeout.code(), None);
    }

    #[test]
    fn a_close_frame_carries_the_code_and_the_label() {
        let frame = CloseReason::AuthDeadline
            .close_frame(None)
            .expect("a frame");
        assert_eq!(u16::from(frame.code), 4408);
        assert_eq!(frame.reason.as_str(), "auth_deadline");
        assert!(CloseReason::HandshakeRejected.close_frame(None).is_none());
    }

    #[test]
    fn an_over_share_close_is_1013_with_a_parseable_retry_hint() {
        let frame = CloseReason::OverShare
            .close_frame(Some(Duration::from_millis(4_250)))
            .expect("a frame");
        assert_eq!(u16::from(frame.code), 1013);
        assert_eq!(frame.reason.as_str(), "over_share;retry_after_ms=4250");
        assert!(frame.reason.len() <= 123);
    }

    #[test]
    fn a_going_away_close_is_1001_and_a_drain_timeout_sends_nothing() {
        let frame = CloseReason::GoingAway.close_frame(None).expect("a frame");
        assert_eq!(u16::from(frame.code), 1001);
        assert_eq!(frame.reason.as_str(), "going_away");
        assert!(CloseReason::GoingAway.sends_close_frame());
        assert!(!CloseReason::DrainTimeout.sends_close_frame());
        assert!(CloseReason::DrainTimeout.close_frame(None).is_none());
    }

    #[test]
    fn labels_are_distinct() {
        let mut labels: Vec<_> = CloseReason::ALL.iter().map(|r| r.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), CloseReason::ALL.len());
    }
}
