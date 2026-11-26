//! [`BackendTag`]: which storage backend minted a zookie.

use core::fmt;

use serde::{Deserialize, Serialize};

/// The storage backend that produced a [`Zookie`](super::Zookie).
///
/// The serialized names cross a wire and are pinned: they name the API (`sql`, `cql`), not the
/// Rust identifiers or the vendor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BackendTag {
    /// The relational store, over the PostgreSQL wire.
    #[serde(rename = "sql")]
    SqlDb,
    /// The wide-column store, over the Cassandra wire.
    #[serde(rename = "cql")]
    CqlDb,
    /// The on-device offline store.
    Offline,
}

impl BackendTag {
    /// The short code used in a zookie's wire form.
    ///
    /// The letters are opaque and must not be re-lettered: swapping `s` and `c` would silently
    /// swap two meanings.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            BackendTag::SqlDb => "c",
            BackendTag::CqlDb => "s",
            BackendTag::Offline => "o",
        }
    }

    /// Parses a wire code back into a tag.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "c" => Some(BackendTag::SqlDb),
            "s" => Some(BackendTag::CqlDb),
            "o" => Some(BackendTag::Offline),
            _ => None,
        }
    }
}

impl fmt::Display for BackendTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            BackendTag::SqlDb => "sql",
            BackendTag::CqlDb => "cql",
            BackendTag::Offline => "offline",
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_round_trips() {
        for tag in [BackendTag::SqlDb, BackendTag::CqlDb, BackendTag::Offline] {
            assert_eq!(BackendTag::from_code(tag.code()), Some(tag));
        }
        assert_eq!(BackendTag::from_code("x"), None);
    }
}
