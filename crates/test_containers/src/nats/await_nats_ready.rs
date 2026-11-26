use std::time::Duration;

use fundamental_nats::{NatsClient, NatsConfig};

use crate::nats::nats_server::IMAGE;
use crate::test_container_error::TestContainerError;

/// How long a NATS server may take to let its readiness probe in.
const READY_BUDGET: Duration = Duration::from_secs(30);

/// Waits until a connection built from `probe` is let in, or reports the last refusal.
///
/// Readiness is an authenticated connection: a server can log ready before it reads its users.
///
/// # Errors
/// [`TestContainerError::NotReady`] when `probe` was never let in within the budget.
pub(crate) async fn await_nats_ready(
    url: &str,
    probe: &NatsConfig,
) -> Result<(), TestContainerError> {
    let deadline = tokio::time::Instant::now() + READY_BUDGET;
    let mut last = String::from("never attempted");
    while tokio::time::Instant::now() < deadline {
        match NatsClient::connect(probe.clone()).await {
            Ok(_) => return Ok(()),
            Err(error) => last = error.to_string(),
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(TestContainerError::NotReady {
        engine: IMAGE,
        endpoint: url.to_owned(),
        waited_secs: READY_BUDGET.as_secs(),
        detail: last,
    })
}
