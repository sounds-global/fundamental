//! [`ConnectionRefusals`]: one connection's refused frames, and its share of warning lines.

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use tokio::time::Instant;
use tracing::warn;

use crate::observability::{FixedWindow, ProcessRefusalBudget, WsStats};

pub(crate) const CONNECTION_WINDOW: Duration = Duration::from_secs(60);
/// Refusal warnings one connection writes per [`CONNECTION_WINDOW`].
pub(crate) const CONNECTION_LINES_PER_WINDOW: u32 = 10;

/// Decides whether each refused frame's warning is written; every suppressed line is reported once, by count.
#[derive(Debug)]
pub(crate) struct ConnectionRefusals {
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    refused: u32,
    lines: FixedWindow,
    suppressed: u64,
}

impl ConnectionRefusals {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            state: Mutex::new(State {
                refused: 0,
                lines: FixedWindow::new(
                    now,
                    CONNECTION_WINDOW,
                    u64::from(CONNECTION_LINES_PER_WINDOW),
                ),
                suppressed: 0,
            }),
        }
    }

    pub(crate) fn count(
        &self,
        now: Instant,
        process: &ProcessRefusalBudget,
        stats: &WsStats,
    ) -> bool {
        let mut state = self.lock();
        state.refused = state.refused.saturating_add(1);
        if !state.lines.permit(now) {
            state.suppressed = state.suppressed.saturating_add(1);
            stats.refusal_suppressed();
            return false;
        }
        if !process.permit(now) {
            stats.refusal_suppressed();
            return false;
        }
        stats.refusal_logged();
        true
    }

    pub(crate) fn refused(&self) -> u32 {
        self.lock().refused
    }

    pub(crate) fn suppressed(&self) -> u64 {
        self.lock().suppressed
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| {
            warn!("connection refusal counter was poisoned by a panic; recovering its counts");
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> (Instant, ProcessRefusalBudget, WsStats) {
        let now = Instant::now();
        (now, ProcessRefusalBudget::new(now), WsStats::new(1, 1, 0))
    }

    #[test]
    fn a_connection_writes_its_allowance_and_carries_the_rest() {
        let (now, process, stats) = fixtures();
        let refusals = ConnectionRefusals::new(now);
        let written = (0..1_000)
            .filter(|_| refusals.count(now, &process, &stats))
            .count();
        assert_eq!(written, CONNECTION_LINES_PER_WINDOW as usize);
        assert_eq!(refusals.refused(), 1_000);
        assert_eq!(refusals.suppressed(), 990);
        assert_eq!(stats.refusal_warnings_logged(), 10);
        assert_eq!(stats.refusal_warnings_suppressed(), 990);
        assert_eq!(process.flush(), 0, "the process budget withheld nothing");
    }

    #[test]
    fn a_new_window_restores_the_allowance() {
        let (now, process, stats) = fixtures();
        let refusals = ConnectionRefusals::new(now);
        for _ in 0..CONNECTION_LINES_PER_WINDOW {
            assert!(refusals.count(now, &process, &stats));
        }
        assert!(!refusals.count(now, &process, &stats));
        assert!(refusals.count(now + CONNECTION_WINDOW, &process, &stats));
    }

    #[test]
    fn the_process_budget_bounds_many_connections() {
        let (now, process, stats) = fixtures();
        let written: usize = (0..50)
            .map(|_| {
                let refusals = ConnectionRefusals::new(now);
                (0..CONNECTION_LINES_PER_WINDOW)
                    .filter(|_| refusals.count(now, &process, &stats))
                    .count()
            })
            .sum();
        assert_eq!(
            written as u64,
            super::super::process_refusal_budget::PROCESS_LINES_PER_WINDOW
        );
        assert_eq!(process.flush(), 500 - written as u64);
    }
}
