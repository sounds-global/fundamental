use std::sync::Arc;
use std::time::Duration;

use crate::{SqlError, SqlResult};

/// Connections one class of work holds for itself, apart from the shared pool, kept open and rotated with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlReservation {
    name: Arc<str>,
    connections: u32,
    acquire_timeout: Duration,
}

impl SqlReservation {
    /// The most connections one reservation may hold; a larger one should be its own pool.
    pub const MAX_CONNECTIONS: u32 = 8;

    /// How long an acquire waits by default before the reservation refuses.
    pub const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(1);

    /// A reservation called `name` holding `connections` connections.
    ///
    /// # Errors
    ///
    /// [`SqlError::Config`] when `name` is not `[a-z][a-z0-9_-]{0,62}` or `connections` is not
    /// between 1 and [`MAX_CONNECTIONS`](Self::MAX_CONNECTIONS).
    pub fn new(name: &str, connections: u32) -> SqlResult<Self> {
        let mut chars = name.chars();
        let named = name.len() <= 63
            && chars.next().is_some_and(|first| first.is_ascii_lowercase())
            && chars
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'));
        if !named {
            return Err(SqlError::Config(format!(
                "a reservation's name is [a-z][a-z0-9_-]{{0,62}}, not `{name}`"
            )));
        }
        if !(1..=Self::MAX_CONNECTIONS).contains(&connections) {
            return Err(SqlError::Config(format!(
                "reservation `{name}` holds 1 to {} connections, not {connections}",
                Self::MAX_CONNECTIONS
            )));
        }
        Ok(Self {
            name: Arc::from(name),
            connections,
            acquire_timeout: Self::DEFAULT_ACQUIRE_TIMEOUT,
        })
    }

    /// Waits at most `timeout` for one of its connections.
    #[must_use]
    pub fn with_acquire_timeout(mut self, timeout: Duration) -> Self {
        self.acquire_timeout = timeout;
        self
    }

    /// What it is called, in its pool's label and its connections' `application_name`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How many connections it holds.
    #[must_use]
    pub fn connections(&self) -> u32 {
        self.connections
    }

    /// How long an acquire waits.
    #[must_use]
    pub fn acquire_timeout(&self) -> Duration {
        self.acquire_timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_reservation_of_a_few_connections_is_accepted() {
        let reservation = SqlReservation::new("deadlines-pulse", 2).expect("in bounds");
        assert_eq!(reservation.name(), "deadlines-pulse");
        assert_eq!(reservation.connections(), 2);
        assert_eq!(
            reservation.acquire_timeout(),
            SqlReservation::DEFAULT_ACQUIRE_TIMEOUT
        );
    }

    #[test]
    fn an_empty_or_oversized_reservation_is_refused() {
        for connections in [0, SqlReservation::MAX_CONNECTIONS + 1] {
            let refused = SqlReservation::new("pulse", connections);
            assert!(
                matches!(refused, Err(SqlError::Config(ref detail)) if detail.contains("connections")),
                "{connections}: {refused:?}"
            );
        }
    }

    #[test]
    fn a_name_out_of_shape_is_refused() {
        for name in ["", "Pulse", "1pulse", "pulse pool", "pulse;drop"] {
            assert!(
                matches!(SqlReservation::new(name, 1), Err(SqlError::Config(_))),
                "{name}"
            );
        }
    }
}
