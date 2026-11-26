//! [`ActivationFault`] — why a passed socket cannot be served.

use std::fmt;

/// Why a socket the service manager passed cannot be served.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivationFault {
    /// The passed sockets are not each named once.
    Malformed(&'static str),
    /// No socket of that name was passed.
    NotPassed,
    /// The socket of that name was already taken by another listener.
    Taken,
    /// The descriptor is not a listening unix stream socket.
    NotAUnixStream(String),
}

impl fmt::Display for ActivationFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(reason) => write!(f, "the passed sockets are malformed: {reason}"),
            Self::NotPassed => f.write_str("no socket of that name was passed"),
            Self::Taken => f.write_str("the socket was already taken"),
            Self::NotAUnixStream(reason) => write!(f, "not a unix stream socket: {reason}"),
        }
    }
}

impl std::error::Error for ActivationFault {}
