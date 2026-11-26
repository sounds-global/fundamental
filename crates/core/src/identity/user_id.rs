//! [`UserId`] — a strongly-typed user identifier.

#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::convert::Infallible;
use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::model::Identifier;

/// A user's identifier, taken from a verified token's `sub` claim.
///
/// The impls are written out as `#[derive(Identifier)]` would emit them: the derive depends on this crate.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(String);

impl UserId {
    /// Wraps a string as a user id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Identifier for UserId {
    type Inner = String;

    fn from_inner(inner: Self::Inner) -> Self {
        Self(inner)
    }

    fn as_inner(&self) -> &Self::Inner {
        &self.0
    }

    fn into_inner(self) -> Self::Inner {
        self.0
    }
}

impl From<String> for UserId {
    fn from(inner: String) -> Self {
        Self(inner)
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for UserId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(String::from(s)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_string() {
        let id = UserId::new("user-42");
        assert_eq!(id.as_str(), "user-42");
        assert_eq!(UserId::new("user-42"), id);
    }

    #[test]
    fn serializes_as_a_plain_string() {
        let id = UserId::new("user-42");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, r#""user-42""#);
    }

    #[test]
    fn the_text_codec_round_trips() {
        let id: UserId = "user-42".parse().expect("infallible");
        assert_eq!(id.to_string(), "user-42");
    }

    #[test]
    fn the_identifier_seam_exposes_the_inner_value() {
        let id = UserId::from_inner(String::from("user-42"));
        assert_eq!(id.as_inner(), "user-42");
        assert_eq!(id.into_inner(), "user-42");
    }
}
