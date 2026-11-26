//! How an HTTP surface's transport security is arranged ([`TlsPosture`]).

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How a surface's transport security is arranged; also loads the legacy `tls_enabled` boolean.
///
/// ```toml
/// api = { port = 16000, tls = "terminated_upstream" }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsPosture {
    /// This listener terminates TLS itself, using its configured certificate and key.
    Enabled,

    /// An ingress terminates TLS and forwards plaintext to this listener.
    TerminatedUpstream,

    /// No TLS anywhere in front of this listener; an error in a deployed environment.
    #[default]
    Disabled,
}

impl TlsPosture {
    /// Whether this listener itself performs the TLS handshake.
    #[must_use]
    pub const fn terminates_tls(self) -> bool {
        matches!(self, Self::Enabled)
    }

    /// Whether traffic reaching a client is encrypted, wherever it is terminated.
    #[must_use]
    pub const fn is_encrypted_in_transit(self) -> bool {
        matches!(self, Self::Enabled | Self::TerminatedUpstream)
    }

    /// The canonical configuration spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::TerminatedUpstream => "terminated_upstream",
            Self::Disabled => "disabled",
        }
    }
}

impl Serialize for TlsPosture {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TlsPosture {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, Unexpected};

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Spelling {
            Bool(bool),
            Text(String),
        }

        match Spelling::deserialize(deserializer)? {
            Spelling::Bool(true) => Ok(Self::Enabled),
            Spelling::Bool(false) => Ok(Self::Disabled),
            Spelling::Text(text) => match text.to_ascii_lowercase().as_str() {
                "enabled" | "true" => Ok(Self::Enabled),
                "terminated_upstream" | "upstream" => Ok(Self::TerminatedUpstream),
                "disabled" | "false" => Ok(Self::Disabled),
                other => Err(D::Error::invalid_value(
                    Unexpected::Str(other),
                    &"`enabled`, `terminated_upstream`, or `disabled`",
                )),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canonical_spellings_round_trip() {
        for posture in [
            TlsPosture::Enabled,
            TlsPosture::TerminatedUpstream,
            TlsPosture::Disabled,
        ] {
            let encoded = toml::Value::try_from(posture).expect("serialize");
            let decoded: TlsPosture = encoded.try_into().expect("deserialize");
            assert_eq!(decoded, posture);
        }
    }

    #[test]
    fn the_legacy_boolean_spelling_still_loads() {
        let enabled: TlsPosture = toml::Value::Boolean(true).try_into().expect("true");
        let disabled: TlsPosture = toml::Value::Boolean(false).try_into().expect("false");

        assert_eq!(enabled, TlsPosture::Enabled);
        assert_eq!(disabled, TlsPosture::Disabled);
    }

    #[test]
    fn an_unknown_spelling_is_rejected_by_name() {
        let error = toml::Value::String("maybe".to_string())
            .try_into::<TlsPosture>()
            .expect_err("an unknown posture must not silently default");

        assert!(
            error.to_string().contains("terminated_upstream"),
            "the error should name the valid spellings: {error}"
        );
    }

    #[test]
    fn only_enabled_terminates_locally_but_upstream_is_still_encrypted() {
        assert!(TlsPosture::Enabled.terminates_tls());
        assert!(!TlsPosture::TerminatedUpstream.terminates_tls());
        assert!(!TlsPosture::Disabled.terminates_tls());

        assert!(TlsPosture::Enabled.is_encrypted_in_transit());
        assert!(TlsPosture::TerminatedUpstream.is_encrypted_in_transit());
        assert!(!TlsPosture::Disabled.is_encrypted_in_transit());
    }
}
