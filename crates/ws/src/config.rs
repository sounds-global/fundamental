//! [`WsConfig`]: configuration for a [`WsServer`](crate::WsServer).

use std::net::SocketAddr;
use std::time::Duration;

use fundamental_core::Validate;
use fundamental_core::validate::ValidationErrors;
use fundamental_macros::Validate;
use tracing::info;

use crate::error::{WsError, WsResult};
use crate::observability::{CONNECTION_LINES_PER_WINDOW, millis};
use crate::session::CLOSE_GRACE;
use crate::slots::{Admission, AnonymousLimits};

/// Configuration for a WebSocket server: where to bind and the per-connection limits.
#[derive(Debug, Clone, Validate)]
pub struct WsConfig {
    /// The address to bind the listener to.
    pub bind: SocketAddr,
    /// The maximum inbound frame size accepted, in bytes.
    #[validate(range(min = 1))]
    pub max_frame_size: usize,
    /// The maximum number of connections served concurrently, anonymous and authenticated together.
    #[validate(range(min = 1))]
    pub max_connections: usize,
    /// Capacity of each connection's outbound channel; a slow client applies backpressure.
    #[validate(range(min = 1))]
    pub send_buffer: usize,
    /// How long the WebSocket upgrade may take before the TCP connection is dropped.
    pub handshake_timeout: Duration,
    /// How long one outbound frame may take to reach the peer before the connection is closed.
    pub send_timeout: Duration,
    /// Whether a connection must authenticate to keep its slot, and the bounds until it does.
    pub admission: Admission,
    /// How many connections one principal may hold bound at once.
    #[validate(range(min = 1))]
    pub max_per_principal: usize,
    /// How many connections the kernel may queue before the server accepts them; a short queue
    /// turns a reconnect storm into a minutes-long tail.
    #[validate(range(min = 1, max = 65535))]
    pub accept_backlog: u32,
    /// How long a stop waits for its connections to finish before aborting the rest; keep it under
    /// the platform's grace period and the runtime's shutdown budget.
    pub drain_timeout: Duration,
    /// How many connections a stop closes per second, at most, so neighbours are not handed the whole population at once.
    #[validate(range(min = 1))]
    pub drain_rate: u32,
}

impl WsConfig {
    /// The longest any connection bound may be.
    pub const MAX_BOUND: Duration = Duration::from_secs(24 * 60 * 60);

    /// Creates a configuration bound to `addr` with conservative default limits.
    #[must_use]
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            bind: addr,
            max_frame_size: 1024 * 1024,
            max_connections: 10_000,
            send_buffer: 256,
            handshake_timeout: Duration::from_secs(5),
            send_timeout: Duration::from_secs(10),
            admission: Admission::default(),
            max_per_principal: 16,
            accept_backlog: 4096,
            drain_timeout: Duration::from_secs(20),
            drain_rate: 1_000,
        }
    }

    /// Sets the maximum inbound frame size, in bytes, also the bound on a reassembled message.
    #[must_use]
    pub fn max_frame_size(mut self, bytes: usize) -> Self {
        self.max_frame_size = bytes;
        self
    }

    /// Sets the maximum number of concurrently served connections.
    #[must_use]
    pub fn max_connections(mut self, count: usize) -> Self {
        self.max_connections = count;
        self
    }

    /// Sets the per-connection outbound channel capacity.
    #[must_use]
    pub fn send_buffer(mut self, capacity: usize) -> Self {
        self.send_buffer = capacity;
        self
    }

    /// Sets how long the WebSocket upgrade may take.
    #[must_use]
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// Sets how long one outbound frame may take to reach the peer.
    #[must_use]
    pub fn send_timeout(mut self, timeout: Duration) -> Self {
        self.send_timeout = timeout;
        self
    }

    /// Sets how many connections one principal may hold bound at once.
    #[must_use]
    pub fn max_per_principal(mut self, count: usize) -> Self {
        self.max_per_principal = count;
        self
    }

    /// Sets how many connections the kernel may queue before the server accepts them.
    #[must_use]
    pub fn accept_backlog(mut self, connections: u32) -> Self {
        self.accept_backlog = connections;
        self
    }

    /// Sets how long a stop waits for its connections to finish.
    #[must_use]
    pub fn drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = timeout;
        self
    }

    /// Sets how many connections a stop closes per second, at most.
    #[must_use]
    pub fn drain_rate(mut self, per_second: u32) -> Self {
        self.drain_rate = per_second;
        self
    }

    /// `open / drain_rate`, within what `drain_timeout` leaves after an upgrade and a goodbye.
    pub(crate) fn drain_spread(&self, open: usize) -> Duration {
        let window = self
            .drain_timeout
            .saturating_sub(self.handshake_timeout)
            .saturating_sub(CLOSE_GRACE * 3);
        let open = u64::try_from(open).unwrap_or(u64::MAX);
        let by_rate =
            Duration::from_millis(open.saturating_mul(1_000) / u64::from(self.drain_rate.max(1)));
        by_rate.min(window)
    }

    /// Sets how connections are admitted.
    #[must_use]
    pub fn admission(mut self, admission: Admission) -> Self {
        self.admission = admission;
        self
    }

    /// Serves every connection without authenticating — shorthand for
    /// [`admission(Admission::Public)`](Self::admission).
    #[must_use]
    pub fn public(self) -> Self {
        self.admission(Admission::Public)
    }

    /// Requires authentication under `limits`.
    #[must_use]
    pub fn authenticate(self, limits: AnonymousLimits) -> Self {
        self.admission(Admission::Authenticate(limits))
    }

    /// Checks every rule a served configuration must keep, naming each one it breaks.
    /// # Errors
    /// [`WsError::InvalidConfig`] carrying every problem found.
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub fn validate(&self) -> WsResult<()> {
        let mut problems = match Validate::validate(self) {
            Ok(()) => ValidationErrors::new(),
            Err(problems) => problems,
        };
        bounded(&mut problems, "handshake_timeout", self.handshake_timeout);
        bounded(&mut problems, "send_timeout", self.send_timeout);
        bounded(&mut problems, "drain_timeout", self.drain_timeout);
        // An upgrade in flight at stop, then its flush, close frame and wait for the peer's, must all fit the drain.
        let upgrade_and_goodbye = self.handshake_timeout.saturating_add(CLOSE_GRACE * 3);
        if !self.drain_timeout.is_zero() && self.drain_timeout < upgrade_and_goodbye {
            problems.add_for_field(
                "drain_timeout",
                "ws.drain_within_handshake",
                format!(
                    "drain_timeout is shorter than handshake_timeout plus {} ms to say goodbye",
                    millis(CLOSE_GRACE * 3)
                ),
            );
        }
        if let Admission::Authenticate(limits) = &self.admission {
            let anonymous = limits.capacity(self.max_connections);
            if anonymous == 0 {
                problems.add_for_field(
                    "admission.max_connections",
                    "ws.anonymous_share_zero",
                    "the anonymous share is zero, so no connection could reach authentication",
                );
            } else if self.max_connections > 0 && anonymous >= self.max_connections {
                problems.add_for_field(
                    "admission.max_connections",
                    "ws.anonymous_share_too_large",
                    format!(
                        "the anonymous share ({anonymous}) leaves no authenticated slot of {}",
                        self.max_connections
                    ),
                );
            }
            bounded(&mut problems, "admission.within", limits.within);
            bounded(
                &mut problems,
                "admission.displace_after",
                limits.displace_after,
            );
            if !limits.displace_after.is_zero() && limits.displace_after >= limits.within {
                problems.add_for_field(
                    "admission.displace_after",
                    "ws.displacement_after_deadline",
                    "displace_after is not shorter than the authentication deadline",
                );
            }
            if limits.max_frame_size == 0 {
                problems.add_for_field(
                    "admission.max_frame_size",
                    "ws.zero",
                    "the anonymous frame size is zero",
                );
            }
            if limits.max_refusals > CONNECTION_LINES_PER_WINDOW {
                problems.add_for_field(
                    "admission.max_refusals",
                    "ws.refusals_outrun_the_log",
                    format!(
                        "more refusals than one connection may log ({CONNECTION_LINES_PER_WINDOW}), \
                         so an anonymous connection's refusals could go unwritten"
                    ),
                );
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(WsError::InvalidConfig { problems })
        }
    }

    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn log_bounds(&self) {
        let handshake_ms = millis(self.handshake_timeout);
        let send_ms = millis(self.send_timeout);
        let drain_ms = millis(self.drain_timeout);
        let drain_rate = self.drain_rate;
        match &self.admission {
            Admission::Authenticate(limits) => info!(
                max_connections = self.max_connections,
                accept_backlog = self.accept_backlog,
                max_per_principal = self.max_per_principal,
                max_anonymous = limits.capacity(self.max_connections),
                max_frame_size = self.max_frame_size,
                anonymous_max_frame_size = limits.max_frame_size,
                anonymous_max_refusals = limits.max_refusals,
                handshake_ms,
                send_ms,
                drain_ms,
                drain_rate,
                authenticate_within_ms = millis(limits.within),
                displace_after_ms = millis(limits.displace_after),
                "websocket bounds: connections must authenticate"
            ),
            Admission::Public => info!(
                max_connections = self.max_connections,
                accept_backlog = self.accept_backlog,
                max_per_principal = self.max_per_principal,
                max_frame_size = self.max_frame_size,
                handshake_ms,
                send_ms,
                drain_ms,
                drain_rate,
                "websocket bounds: a public socket, served without authentication"
            ),
        }
    }
}

fn bounded(problems: &mut ValidationErrors, field: &str, duration: Duration) {
    if duration.is_zero() {
        problems.add_for_field(field, "ws.zero", format!("{field} is zero"));
    } else if duration > WsConfig::MAX_BOUND {
        problems.add_for_field(
            field,
            "ws.too_long",
            format!("{field} is longer than a day"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    fn addr() -> SocketAddr {
        "127.0.0.1:0".parse().expect("valid addr")
    }

    fn codes(config: &WsConfig, field: &str) -> Vec<String> {
        let Err(WsError::InvalidConfig { problems }) = config.validate() else {
            return Vec::new();
        };
        problems
            .errors_for_field(field)
            .iter()
            .map(|problem| problem.code.clone())
            .collect()
    }

    #[test]
    fn defaults_are_conservative() {
        let config = WsConfig::new(addr());
        assert_eq!(config.max_frame_size, 1024 * 1024);
        assert_eq!(config.max_connections, 10_000);
        assert_eq!(config.handshake_timeout, Duration::from_secs(5));
        assert_eq!(config.send_timeout, Duration::from_secs(10));
        assert_eq!(config.max_per_principal, 16);
        assert_eq!(config.accept_backlog, 4096);
        assert_eq!(config.drain_timeout, Duration::from_secs(20));
        assert_eq!(config.admission, Admission::default());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn builders_set_limits() {
        let config = WsConfig::new(addr())
            .max_frame_size(4096)
            .max_connections(16)
            .handshake_timeout(Duration::from_secs(1))
            .send_timeout(Duration::from_secs(2))
            .max_per_principal(3)
            .public();
        assert_eq!(config.max_frame_size, 4096);
        assert_eq!(config.max_connections, 16);
        assert_eq!(config.handshake_timeout, Duration::from_secs(1));
        assert_eq!(config.send_timeout, Duration::from_secs(2));
        assert_eq!(config.max_per_principal, 3);
        assert_eq!(config.admission, Admission::Public);
    }

    #[test]
    fn invalid_config_reports_every_problem() {
        let config = WsConfig::new(addr())
            .max_connections(4)
            .send_buffer(0)
            .send_timeout(Duration::ZERO)
            .authenticate(
                AnonymousLimits::default()
                    .max_connections(4)
                    .displace_after(Duration::from_secs(10)),
            );
        let error = config.validate().expect_err("four problems");
        assert_eq!(error.error_code(), "WS_010");
        assert!(!codes(&config, "send_buffer").is_empty());
        assert_eq!(codes(&config, "send_timeout"), ["ws.zero"]);
        assert_eq!(
            codes(&config, "admission.max_connections"),
            ["ws.anonymous_share_too_large"]
        );
        assert_eq!(
            codes(&config, "admission.displace_after"),
            ["ws.displacement_after_deadline"]
        );
    }

    #[test]
    fn a_deadline_longer_than_a_day_is_refused() {
        let config = WsConfig::new(addr())
            .handshake_timeout(Duration::MAX)
            .authenticate(
                AnonymousLimits::default()
                    .within(Duration::MAX)
                    .displace_after(WsConfig::MAX_BOUND + Duration::from_secs(1)),
            );
        assert_eq!(codes(&config, "handshake_timeout"), ["ws.too_long"]);
        assert_eq!(codes(&config, "admission.within"), ["ws.too_long"]);
        assert_eq!(codes(&config, "admission.displace_after"), ["ws.too_long"]);
    }

    #[test]
    fn a_drain_that_cannot_fit_an_upgrade_and_its_goodbye_is_refused() {
        let config = WsConfig::new(addr())
            .handshake_timeout(Duration::from_secs(5))
            .drain_timeout(Duration::from_millis(7_999));
        assert_eq!(
            codes(&config, "drain_timeout"),
            ["ws.drain_within_handshake"]
        );
        let fits = config.drain_timeout(Duration::from_secs(8));
        assert!(fits.validate().is_ok());
    }

    #[test]
    fn a_drain_spreads_by_its_rate_within_its_window() {
        let config = WsConfig::new(addr()).drain_rate(1_000);
        assert_eq!(config.drain_spread(3), Duration::from_millis(3));
        assert_eq!(config.drain_spread(5_000), Duration::from_secs(5));
        assert_eq!(config.drain_spread(1_000_000), Duration::from_secs(12));
        assert!(!codes(&config.drain_rate(0), "drain_rate").is_empty());
    }

    #[test]
    fn a_zero_or_endless_drain_is_refused() {
        let zero = WsConfig::new(addr()).drain_timeout(Duration::ZERO);
        assert_eq!(codes(&zero, "drain_timeout"), ["ws.zero"]);
        let endless = WsConfig::new(addr()).drain_timeout(WsConfig::MAX_BOUND * 2);
        assert_eq!(codes(&endless, "drain_timeout"), ["ws.too_long"]);
    }

    #[test]
    fn more_anonymous_refusals_than_a_connection_may_log_are_refused() {
        let config = WsConfig::new(addr())
            .authenticate(AnonymousLimits::default().max_refusals(CONNECTION_LINES_PER_WINDOW + 1));
        assert_eq!(
            codes(&config, "admission.max_refusals"),
            ["ws.refusals_outrun_the_log"]
        );
    }

    #[test]
    fn a_zero_anonymous_share_is_refused() {
        let config =
            WsConfig::new(addr()).authenticate(AnonymousLimits::default().max_connections(0));
        assert_eq!(
            codes(&config, "admission.max_connections"),
            ["ws.anonymous_share_zero"]
        );
    }

    #[test]
    fn a_server_with_no_connections_or_principals_is_refused() {
        let config = WsConfig::new(addr())
            .max_connections(0)
            .max_per_principal(0)
            .public();
        assert!(!codes(&config, "max_connections").is_empty());
        assert!(!codes(&config, "max_per_principal").is_empty());
    }
}
