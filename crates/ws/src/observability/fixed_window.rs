//! [`FixedWindow`]: an allowance that renews every window.

use std::time::Duration;

use tokio::time::Instant;

/// Admits `limit` events per `window`, starting a new window on the first event after the last ended.
#[derive(Debug)]
pub(crate) struct FixedWindow {
    window: Duration,
    limit: u64,
    started: Instant,
    used: u64,
}

impl FixedWindow {
    pub(crate) fn new(now: Instant, window: Duration, limit: u64) -> Self {
        Self {
            window,
            limit,
            started: now,
            used: 0,
        }
    }

    pub(crate) fn permit(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.started) >= self.window {
            self.started = now;
            self.used = 0;
        }
        if self.used >= self.limit {
            return false;
        }
        self.used += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(10);

    #[test]
    fn a_window_admits_its_limit_and_no_more() {
        let now = Instant::now();
        let mut window = FixedWindow::new(now, WINDOW, 3);
        let admitted = (0..10).filter(|_| window.permit(now)).count();
        assert_eq!(admitted, 3);
    }

    #[test]
    fn the_next_window_renews_the_allowance() {
        let now = Instant::now();
        let mut window = FixedWindow::new(now, WINDOW, 1);
        assert!(window.permit(now));
        assert!(!window.permit(now + WINDOW - Duration::from_millis(1)));
        assert!(window.permit(now + WINDOW));
    }
}
