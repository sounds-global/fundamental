//! [`SocketName`] — the name a passed socket carries.

use std::fmt;

/// The name a passed socket carries (systemd's `FileDescriptorName`): `[a-z][a-z0-9_-]{0,63}`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SocketName(String);

impl SocketName {
    /// `raw` as a name, or `None` when it is not one.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let mut bytes = raw.bytes();
        let first = bytes.next()?;
        let rest = |byte: u8| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        };
        (first.is_ascii_lowercase() && raw.len() <= 64 && bytes.all(rest))
            .then(|| Self(raw.to_owned()))
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SocketName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_surface_name_is_a_socket_name() {
        for name in ["web", "admin", "web-2", "a_b"] {
            assert_eq!(
                SocketName::parse(name).map(|name| name.to_string()),
                Some(name.to_owned())
            );
        }
    }

    #[test]
    fn a_socket_name_refuses_a_colon_space_and_empty() {
        for raw in [
            "",
            "web:admin",
            "web admin",
            "Web",
            "2web",
            "-web",
            &"a".repeat(65),
        ] {
            assert_eq!(SocketName::parse(raw), None, "{raw:?}");
        }
        assert!(SocketName::parse(&"a".repeat(64)).is_some());
    }
}
