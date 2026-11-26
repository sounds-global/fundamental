//! [`DeviceId`] — a strongly-typed device identifier.

#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::convert::Infallible;
use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::model::Identifier;

/// A device's stable identifier, bound to a user at enrollment.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(String);

impl DeviceId {
    /// Wraps a string as a device id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Identifier for DeviceId {
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

impl From<String> for DeviceId {
    fn from(inner: String) -> Self {
        Self(inner)
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for DeviceId {
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
        let id = DeviceId::new("device-7");
        assert_eq!(id.as_str(), "device-7");
        assert_eq!(DeviceId::new("device-7"), id);
    }

    #[test]
    fn serializes_as_a_plain_string() {
        let id = DeviceId::new("device-7");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, r#""device-7""#);
    }

    #[test]
    fn the_text_codec_round_trips() {
        let id: DeviceId = "device-7".parse().expect("infallible");
        assert_eq!(id.to_string(), "device-7");
    }

    #[test]
    fn the_identifier_seam_exposes_the_inner_value() {
        let id = DeviceId::from_inner(String::from("device-7"));
        assert_eq!(id.as_inner(), "device-7");
        assert_eq!(id.into_inner(), "device-7");
    }
}
