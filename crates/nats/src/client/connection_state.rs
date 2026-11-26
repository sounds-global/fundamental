use std::fmt;

/// The connection state of a [`NatsClient`](crate::NatsClient).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionState {
    /// Not connected.
    Disconnected,
    /// Establishing the initial connection.
    Connecting,
    /// Connected and ready.
    Connected,
    /// Reconnecting after a drop.
    Reconnecting,
    /// Closed and will not reconnect.
    Closed,
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
            Self::Closed => "closed",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_lowercase() {
        assert_eq!(ConnectionState::Connected.to_string(), "connected");
        assert_eq!(ConnectionState::Disconnected.to_string(), "disconnected");
        assert_eq!(ConnectionState::Reconnecting.to_string(), "reconnecting");
    }
}
