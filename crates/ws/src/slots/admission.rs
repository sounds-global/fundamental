//! [`Admission`]: whether a server's connections must authenticate to keep their slot.

use crate::slots::AnonymousLimits;

/// How a [`WsServer`](crate::WsServer) admits a connection; [`Authenticate`](Self::Authenticate) unless a socket chooses [`Public`](Self::Public).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// A connection is anonymous, held to [`AnonymousLimits`], until a handler binds a principal with
    /// [`WsContext::bind_principal`](crate::WsContext::bind_principal).
    Authenticate(AnonymousLimits),
    /// Every connection is served without authenticating, from one pool of `max_connections`.
    Public,
}

impl Default for Admission {
    fn default() -> Self {
        Self::Authenticate(AnonymousLimits::default())
    }
}

impl Admission {
    pub(crate) fn anonymous(&self) -> Option<&AnonymousLimits> {
        match self {
            Self::Authenticate(limits) => Some(limits),
            Self::Public => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_requires_authentication() {
        assert_eq!(
            Admission::default(),
            Admission::Authenticate(AnonymousLimits::default())
        );
        assert!(Admission::default().anonymous().is_some());
    }

    #[test]
    fn a_public_socket_has_no_anonymous_bounds() {
        assert!(Admission::Public.anonymous().is_none());
    }
}
