//! [`StatusBoard`] — where a sync session publishes its [`ConnectionStatus`].

use std::sync::Arc;

use tokio::sync::watch;
use tracing::info;
use web_time::Instant;

use crate::{ConnectionStatus, ConnectionWatch};

/// A status and when the session entered it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Observed {
    pub(crate) status: ConnectionStatus,
    since: Instant,
}

/// The publishing half of a session's status, shared by the session and its handle.
///
/// `Stopped` is absorbing, so a session unwinding after its handle dropped cannot report itself alive.
#[derive(Debug, Clone)]
pub(crate) struct StatusBoard {
    sender: Arc<watch::Sender<Observed>>,
}

impl StatusBoard {
    /// A board reading `Connecting`.
    pub(crate) fn new() -> Self {
        let (sender, _receiver) = watch::channel(Observed {
            status: ConnectionStatus::Connecting,
            since: Instant::now(),
        });
        Self {
            sender: Arc::new(sender),
        }
    }

    /// A new view of the status.
    pub(crate) fn watch(&self) -> ConnectionWatch {
        ConnectionWatch::new(self.sender.subscribe())
    }

    /// Moves the session to `status`; a repeat, or anything after `Stopped`, changes nothing.
    pub(crate) fn publish(&self, status: ConnectionStatus) {
        let now = Instant::now();
        self.sender.send_if_modified(|observed| {
            let previous = observed.status;
            if previous == ConnectionStatus::Stopped || previous == status {
                return false;
            }
            info!(
                from = ?previous,
                to = ?status,
                in_previous_ms = now.saturating_duration_since(observed.since).as_millis(),
                "sync connection status changed"
            );
            *observed = Observed { status, since: now };
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WaitCause;

    #[test]
    fn stopped_is_final() {
        let board = StatusBoard::new();
        board.publish(ConnectionStatus::Live);
        board.publish(ConnectionStatus::Stopped);
        board.publish(ConnectionStatus::Connecting);
        board.publish(ConnectionStatus::Waiting {
            until: Instant::now(),
            cause: WaitCause::Closed,
        });
        assert_eq!(board.watch().current(), ConnectionStatus::Stopped);
    }

    #[test]
    fn a_watch_sees_the_current_status_and_each_change() {
        let board = StatusBoard::new();
        let watch = board.watch();
        assert_eq!(watch.current(), ConnectionStatus::Connecting);
        board.publish(ConnectionStatus::Live);
        assert_eq!(watch.current(), ConnectionStatus::Live);
    }
}
