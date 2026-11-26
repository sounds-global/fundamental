use fundamental_core::AnyError;
use fundamental_core::listen::Listen;
use fundamental_server::{ServerError, ServerListener};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::info;

/// A handle to a running axum server: its bound address and the controls to stop it.
pub struct ApiListener {
    local_addr: Listen,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl ApiListener {
    pub(crate) fn new(
        local_addr: Listen,
        shutdown: oneshot::Sender<()>,
        task: JoinHandle<()>,
    ) -> Self {
        Self {
            local_addr,
            shutdown: Some(shutdown),
            task,
        }
    }
}

impl ServerListener for ApiListener {
    fn local_addr(&self) -> &Listen {
        &self.local_addr
    }

    async fn stop(mut self) -> Result<(), AnyError> {
        info!(addr = %self.local_addr, "stopping api server");
        // A closed receiver means the server already began stopping.
        if let Some(trigger) = self.shutdown.take() {
            let _ = trigger.send(());
        }
        self.task.await.map_err(|join_error| {
            AnyError::new(ServerError::internal(format!(
                "api server task did not stop cleanly: {join_error}"
            )))
        })
    }
}
