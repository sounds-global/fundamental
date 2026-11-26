//! [`drain_sessions`]: ends every session with `1001`, and aborts what outlives the drain.

use tokio::task::JoinSet;
use tokio::time::{Instant, timeout};
use tracing::{error, info, warn};

use crate::config::WsConfig;
use crate::observability::millis;
use crate::session::{CLOSE_GRACE, CloseReason};
use crate::slots::AdmissionControl;

/// Tells every session to drain across the spread, waits up to `drain_timeout`, then aborts the rest;
/// the aborted ones get one close grace, since an abort is cooperative.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) async fn drain_sessions(
    mut sessions: JoinSet<()>,
    control: &AdmissionControl,
    config: &WsConfig,
) {
    let started = Instant::now();
    let drain_timeout = config.drain_timeout;
    let open = sessions.len();
    let spread = config.drain_spread(open);
    let stats = control.stats();
    let going_away_before = stats.closes(CloseReason::GoingAway);
    info!(
        open,
        spread_ms = millis(spread),
        drain_timeout_ms = millis(drain_timeout),
        "websocket drain started"
    );
    control.drain().begin(spread);

    let mut aborted = 0;
    let mut still_running = 0;
    if timeout(drain_timeout, join_all(&mut sessions))
        .await
        .is_err()
    {
        aborted = sessions.len();
        control.drain().expire();
        sessions.abort_all();
        if timeout(CLOSE_GRACE, join_all(&mut sessions)).await.is_err() {
            still_running = sessions.len();
            sessions.detach_all();
        }
        warn!(
            aborted,
            still_running,
            drain_timeout_ms = millis(drain_timeout),
            "websocket drain timed out; aborted the sessions still running"
        );
    }
    info!(
        open,
        going_away = stats.closes(CloseReason::GoingAway) - going_away_before,
        finished = open - aborted,
        aborted,
        still_running,
        elapsed_ms = millis(started.elapsed()),
        "websocket drain finished"
    );
}

async fn join_all(sessions: &mut JoinSet<()>) {
    while let Some(ended) = sessions.join_next().await {
        if let Err(error) = ended
            && error.is_panic()
        {
            error!(%error, "a connection's session panicked");
        }
    }
}
