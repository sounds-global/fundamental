//! [`Pool`]: which of a server's slot pools a connection's permit came from.

/// The pool a connection's one permit was taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pool {
    /// A connection that has not bound a principal, on a server that requires one.
    Anonymous,
    /// A connection that has bound a principal.
    Authenticated,
    /// A connection on a public socket, which never needs to bind one.
    Public,
    /// A connection over the anonymous share, upgraded only to be closed with a retry hint.
    Refusing,
}

impl Pool {
    /// Every pool, in the order a gauge reports them.
    pub(crate) const ALL: [Self; 4] = [
        Self::Anonymous,
        Self::Authenticated,
        Self::Public,
        Self::Refusing,
    ];

    /// The pool's metric label.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Anonymous => "anonymous",
            Self::Authenticated => "authenticated",
            Self::Public => "public",
            Self::Refusing => "refusing",
        }
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Anonymous => 0,
            Self::Authenticated => 1,
            Self::Public => 2,
            Self::Refusing => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_numbers_all_in_order() {
        for (position, pool) in Pool::ALL.iter().enumerate() {
            assert_eq!(pool.index(), position);
        }
    }
}
