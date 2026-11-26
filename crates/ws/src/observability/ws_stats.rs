//! [`WsStats`]: a server's connections and closes, as atomics read at scrape time.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::session::CloseReason;
use crate::slots::Pool;

/// What a [`WsServer`](crate::WsServer) holds and has closed, counted on permit take and give-back.
#[derive(Debug)]
pub struct WsStats {
    open: [AtomicU64; Pool::ALL.len()],
    capacity: [u64; Pool::ALL.len()],
    closes: [AtomicU64; CloseReason::ALL.len()],
    refusal_warnings_logged: AtomicU64,
    refusal_warnings_suppressed: AtomicU64,
}

impl WsStats {
    pub(crate) fn new(anonymous: usize, authenticated: usize, public: usize) -> Self {
        Self {
            open: Default::default(),
            capacity: [anonymous, authenticated, public, anonymous].map(|slots| slots as u64),
            closes: Default::default(),
            refusal_warnings_logged: AtomicU64::new(0),
            refusal_warnings_suppressed: AtomicU64::new(0),
        }
    }

    /// Anonymous connections open right now: accepted, and no principal bound yet.
    #[must_use]
    pub fn anonymous(&self) -> u64 {
        self.open(Pool::Anonymous)
    }

    /// Connections open right now with a principal bound.
    #[must_use]
    pub fn authenticated(&self) -> u64 {
        self.open(Pool::Authenticated)
    }

    /// Connections open right now on a public socket.
    #[must_use]
    pub fn public(&self) -> u64 {
        self.open(Pool::Public)
    }

    /// Connections open only to be closed with a retry hint, because the anonymous share was full.
    #[must_use]
    pub fn refusing(&self) -> u64 {
        self.open(Pool::Refusing)
    }

    /// Connections closed for `reason` since the server started.
    #[must_use]
    pub fn closes(&self, reason: CloseReason) -> u64 {
        self.closes[reason.index()].load(Ordering::Relaxed)
    }

    /// Refusal warnings written to the log since the server started.
    #[must_use]
    pub fn refusal_warnings_logged(&self) -> u64 {
        self.refusal_warnings_logged.load(Ordering::Relaxed)
    }

    /// Refusal warnings withheld by a budget since the server started.
    #[must_use]
    pub fn refusal_warnings_suppressed(&self) -> u64 {
        self.refusal_warnings_suppressed.load(Ordering::Relaxed)
    }

    pub(crate) fn open(&self, pool: Pool) -> u64 {
        self.open[pool.index()].load(Ordering::Relaxed)
    }

    pub(crate) fn capacity(&self, pool: Pool) -> u64 {
        self.capacity[pool.index()]
    }

    pub(crate) fn opened(&self, pool: Pool) {
        self.open[pool.index()].fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn released(&self, pool: Pool) {
        self.open[pool.index()].fetch_sub(1, Ordering::Relaxed);
    }

    pub(crate) fn closed(&self, reason: CloseReason) {
        self.closes[reason.index()].fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn refusal_logged(&self) {
        self.refusal_warnings_logged.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn refusal_suppressed(&self) {
        self.refusal_warnings_suppressed
            .fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_and_releasing_nets_to_zero() {
        let stats = WsStats::new(2, 8, 0);
        stats.opened(Pool::Anonymous);
        stats.opened(Pool::Anonymous);
        stats.released(Pool::Anonymous);
        stats.opened(Pool::Authenticated);
        assert_eq!(stats.anonymous(), 1);
        assert_eq!(stats.authenticated(), 1);
        stats.released(Pool::Anonymous);
        stats.released(Pool::Authenticated);
        assert_eq!(stats.anonymous(), 0);
        assert_eq!(stats.authenticated(), 0);
    }

    #[test]
    fn closes_are_counted_by_reason() {
        let stats = WsStats::new(1, 1, 0);
        stats.closed(CloseReason::AuthDeadline);
        stats.closed(CloseReason::AuthDeadline);
        stats.closed(CloseReason::SlowPeer);
        assert_eq!(stats.closes(CloseReason::AuthDeadline), 2);
        assert_eq!(stats.closes(CloseReason::SlowPeer), 1);
        assert_eq!(stats.closes(CloseReason::Displaced), 0);
    }

    #[test]
    fn capacity_is_what_the_server_was_built_with() {
        let stats = WsStats::new(3, 7, 0);
        assert_eq!(stats.capacity(Pool::Anonymous), 3);
        assert_eq!(stats.capacity(Pool::Authenticated), 7);
        assert_eq!(stats.capacity(Pool::Public), 0);
    }
}
