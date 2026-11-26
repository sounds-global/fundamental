//! [`AnonymousLimits`]: what a connection that has proved nothing may hold, and for how long.

use std::time::Duration;

/// The default anonymous share, in percent: the smallest at which a ten-thousand-device storm binds
/// in twice the half-server time (`examples/reconnect_storm.rs` measures it).
pub(crate) const DEFAULT_ANONYMOUS_PERCENT: usize = 10;

/// The bounds on a connection between its TCP accept and the moment a principal is bound to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnonymousLimits {
    /// How long after its accept a connection has to bind a principal before it is closed with `4408`.
    pub within: Duration,
    /// How many anonymous connections are held at once; `None` reserves 10 % of `max_connections`, at least one.
    pub max_connections: Option<usize>,
    /// How long an anonymous connection is guaranteed before it may be displaced; set it above a real
    /// device's p99 handshake.
    pub displace_after: Duration,
    /// The largest text frame an anonymous connection may send, checked before it is parsed.
    pub max_frame_size: usize,
    /// How many frames an anonymous connection may have refused before it is closed with `1008`.
    pub max_refusals: u32,
}

impl Default for AnonymousLimits {
    /// 10 s to authenticate, 5 s before displacement, 16 KiB frames and eight refusals.
    fn default() -> Self {
        Self {
            within: Duration::from_secs(10),
            max_connections: None,
            displace_after: Duration::from_secs(5),
            max_frame_size: 16 * 1024,
            max_refusals: 8,
        }
    }
}

impl AnonymousLimits {
    /// Sets how long a connection has to bind a principal.
    #[must_use]
    pub fn within(mut self, within: Duration) -> Self {
        self.within = within;
        self
    }

    /// Sets how many anonymous connections are held at once.
    #[must_use]
    pub fn max_connections(mut self, count: usize) -> Self {
        self.max_connections = Some(count);
        self
    }

    /// Sets how long an anonymous connection is guaranteed before it may be displaced.
    #[must_use]
    pub fn displace_after(mut self, after: Duration) -> Self {
        self.displace_after = after;
        self
    }

    /// Sets the largest text frame an anonymous connection may send.
    #[must_use]
    pub fn max_frame_size(mut self, bytes: usize) -> Self {
        self.max_frame_size = bytes;
        self
    }

    /// Sets how many refused frames close an anonymous connection.
    #[must_use]
    pub fn max_refusals(mut self, count: u32) -> Self {
        self.max_refusals = count;
        self
    }

    pub(crate) fn capacity(&self, max_connections: usize) -> usize {
        self.max_connections
            .unwrap_or_else(|| (max_connections * DEFAULT_ANONYMOUS_PERCENT / 100).max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_share_is_a_percentage_of_the_server() {
        let limits = AnonymousLimits::default();
        assert_eq!(
            limits.capacity(10_000),
            10_000 * DEFAULT_ANONYMOUS_PERCENT / 100
        );
    }

    #[test]
    fn a_tiny_server_still_admits_one_anonymous_connection() {
        assert_eq!(AnonymousLimits::default().capacity(3), 1);
    }

    #[test]
    fn an_explicit_share_wins() {
        assert_eq!(
            AnonymousLimits::default().max_connections(7).capacity(100),
            7
        );
    }

    #[test]
    fn builders_set_every_bound() {
        let limits = AnonymousLimits::default()
            .within(Duration::from_secs(3))
            .displace_after(Duration::from_secs(1))
            .max_frame_size(512)
            .max_refusals(2);
        assert_eq!(limits.within, Duration::from_secs(3));
        assert_eq!(limits.displace_after, Duration::from_secs(1));
        assert_eq!(limits.max_frame_size, 512);
        assert_eq!(limits.max_refusals, 2);
    }
}
