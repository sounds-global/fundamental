//! [`ConnectionWatch`] — an app's view of a sync session's [`ConnectionStatus`].

use tokio::sync::watch;

use crate::ConnectionStatus;
use crate::status_board::Observed;

/// The current [`ConnectionStatus`] of a [`SyncClient`](crate::SyncClient), and every change after it.
///
/// ```no_run
/// # extern crate fundamental_ws_client;
/// # async fn show(client: &fundamental_ws_client::SyncClient) {
/// use fundamental_ws_client::ConnectionStatus;
///
/// let mut watch = client.status();
/// while let Some(status) = watch.changed().await {
///     let offline = !matches!(status, ConnectionStatus::Live);
///     // show or hide the app's "offline" indicator
/// #   let _ = offline;
/// }
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct ConnectionWatch {
    receiver: watch::Receiver<Observed>,
}

impl ConnectionWatch {
    pub(crate) fn new(receiver: watch::Receiver<Observed>) -> Self {
        Self { receiver }
    }

    /// The status now.
    #[must_use]
    pub fn current(&self) -> ConnectionStatus {
        self.receiver.borrow().status
    }

    /// Waits for the next status, or `None` once the session is gone; a slow reader sees the newest.
    pub async fn changed(&mut self) -> Option<ConnectionStatus> {
        match self.receiver.changed().await {
            Ok(()) => Some(self.receiver.borrow_and_update().status),
            Err(_closed) => None,
        }
    }
}
