//! [`until`] — wait for a server to have done something it is bound to do.

use std::time::Duration;

use tokio::time::{Instant, sleep};

/// How long a suite waits for anything the server is bound to do.
pub const PATIENCE: Duration = Duration::from_secs(20);

/// Waits until `condition` holds, polling every ten milliseconds.
///
/// # Panics
/// Naming `what`, if it has not held within twenty seconds.
pub async fn until(condition: impl Fn() -> bool, what: &str) {
    let started = Instant::now();
    while !condition() {
        assert!(started.elapsed() < PATIENCE, "never happened: {what}");
        sleep(Duration::from_millis(10)).await;
    }
}
