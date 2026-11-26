//! [`HandlePrefix`] — the kind a handle names, spelled as a TypeID prefix.

use core::fmt;

/// A TypeID v0.3 prefix: at most 63 characters of `[a-z_]`, starting and ending with a letter.
///
/// Only the `const` [`new`](Self::new) builds one, so a prefix in source is checked when it compiles.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HandlePrefix(&'static str);

impl HandlePrefix {
    /// The longest prefix TypeID allows.
    pub const MAX_LEN: usize = 63;

    /// The empty prefix: a bare TypeID suffix with no separator.
    pub const EMPTY: Self = Self("");

    /// The prefix `text`, if it is one.
    #[must_use]
    pub const fn new(text: &'static str) -> Option<Self> {
        if is_type_id_prefix(text) {
            Some(Self(text))
        } else {
            None
        }
    }

    /// The prefix as written.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

/// Whether `text` matches TypeID's `^([a-z]([a-z_]{0,61}[a-z])?)?$`.
pub(crate) const fn is_type_id_prefix(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() > HandlePrefix::MAX_LEN {
        return false;
    }
    if bytes.is_empty() {
        return true;
    }
    if !bytes[0].is_ascii_lowercase() || !bytes[bytes.len() - 1].is_ascii_lowercase() {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        if !(bytes[index].is_ascii_lowercase() || bytes[index] == b'_') {
            return false;
        }
        index += 1;
    }
    true
}

impl fmt::Display for HandlePrefix {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl fmt::Debug for HandlePrefix {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "HandlePrefix({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::HandlePrefix;

    #[test]
    fn typeid_prefixes_are_accepted() {
        for prefix in ["a", "team", "build_request", "pre__fix", ""] {
            assert!(HandlePrefix::new(prefix).is_some(), "{prefix}");
        }
        let longest = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijk";
        assert_eq!(longest.len(), 63);
        assert!(HandlePrefix::new(longest).is_some());
    }

    #[test]
    fn everything_else_is_refused() {
        for prefix in [
            "Team",
            "team1",
            "_team",
            "team_",
            "té",
            "pre.fix",
            " team",
            "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijkl",
        ] {
            assert!(HandlePrefix::new(prefix).is_none(), "{prefix}");
        }
    }

    #[test]
    fn a_prefix_is_checked_when_it_compiles() {
        const TEAM: Option<HandlePrefix> = HandlePrefix::new("team");
        assert_eq!(TEAM.map(|prefix| prefix.as_str()), Some("team"));
    }
}
