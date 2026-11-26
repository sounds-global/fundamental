//! [`SecretRef`]: a secret written in configuration, or where to read it.

use std::fmt;

use fundamental_core::secrets::SecretString;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::VaultKvRef;

/// A secret setting: the value itself (serialised `[REDACTED]`), or where the secret store holds it.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SecretRef {
    /// The secret itself, as configuration supplied it.
    Value(SecretString),
    /// A field of a key-value (version 2) secret in the secret store.
    VaultKv(VaultKvRef),
}

impl SecretRef {
    /// Whether this is a reference, which only the secret store can resolve.
    #[must_use]
    pub fn is_reference(&self) -> bool {
        matches!(self, Self::VaultKv(_))
    }

    /// Whether this is a value configuration supplied as the empty string (an unset `${VAR:-}`).
    #[must_use]
    pub fn is_blank_value(&self) -> bool {
        matches!(self, Self::Value(value) if value.expose().is_empty())
    }
}

impl<'de> Deserialize<'de> for SecretRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SecretRefVisitor)
    }
}

/// Reads a string as a value and a table as a reference, so a malformed table says what is wrong.
struct SecretRefVisitor;

impl<'de> Visitor<'de> for SecretRefVisitor {
    type Value = SecretRef;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a secret as a string, or a reference as `{ source = \"vault_kv\", mount, path, key }`",
        )
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<SecretRef, E> {
        Ok(SecretRef::Value(SecretString::new(value.to_owned())))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<SecretRef, E> {
        Ok(SecretRef::Value(SecretString::new(value)))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<SecretRef, A::Error> {
        VaultKvRef::deserialize(de::value::MapAccessDeserializer::new(map)).map(SecretRef::VaultKv)
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::SecretRef;
    use crate::settings::VaultKvRef;

    #[derive(Debug, Deserialize)]
    struct Holder {
        secret: SecretRef,
    }

    fn parsed(toml: &str) -> Result<SecretRef, toml::de::Error> {
        toml::from_str::<Holder>(toml).map(|holder| holder.secret)
    }

    #[test]
    fn a_string_is_the_value_itself() {
        let SecretRef::Value(value) = parsed("secret = \"pw\"").expect("a value") else {
            panic!("a string is a value");
        };
        assert_eq!(value.expose(), "pw");
    }

    #[test]
    fn a_table_is_a_reference_to_the_store() {
        let reference = parsed(
            "secret = { source = \"vault_kv\", mount = \"a7d\", path = \"nats/mijn\", key = \"password\" }",
        )
        .expect("a reference");
        let SecretRef::VaultKv(kv) = &reference else {
            panic!("a table is a reference: {reference:?}");
        };
        assert_eq!(
            kv,
            &VaultKvRef {
                mount: "a7d".into(),
                path: "nats/mijn".into(),
                key: "password".into(),
            }
        );
        assert!(reference.is_reference());
        assert_eq!(kv.to_string(), "vault_kv a7d/nats/mijn#password");
    }

    #[test]
    fn a_malformed_reference_is_refused_for_what_is_wrong_with_it() {
        for (toml, why) in [
            (
                "secret = { source = \"vault_dynamic\", mount = \"m\", path = \"p\", key = \"k\" }",
                "unknown variant `vault_dynamic`",
            ),
            (
                "secret = { mount = \"m\", path = \"p\", key = \"k\" }",
                "missing field `source`",
            ),
            (
                "secret = { source = \"vault_kv\", mount = \"m\", path = \"p\" }",
                "missing field `key`",
            ),
            (
                "secret = { source = \"vault_kv\", mount = \"m\", path = \"p\", key = \"k\", version = 2 }",
                "unknown field `version`",
            ),
            (
                "secret = { source = \"vault_kv\", mount = \"\", path = \"nats/../other\", key = \"k\" }",
                "its mount and path are not",
            ),
            (
                "secret = { source = \"vault_kv\", mount = \"secret\", path = \"nats/app\", key = \"\" }",
                "its key is not",
            ),
            ("secret = 42", "a secret as a string, or a reference"),
        ] {
            let refused = parsed(toml).expect_err(toml);
            assert!(refused.to_string().contains(why), "{toml}: {refused}");
        }
    }

    #[test]
    fn a_value_never_serialises_and_a_reference_serialises_as_written() {
        #[derive(serde::Serialize)]
        struct Dumped {
            secret: SecretRef,
        }

        let value = parsed("secret = \"hunter2\"").expect("a value");
        assert!(!format!("{value:?}").contains("hunter2"));
        let dumped = toml::to_string(&Dumped { secret: value }).expect("serialisable");
        assert_eq!(dumped.trim(), "secret = \"[REDACTED]\"");

        let written =
            "secret = { source = \"vault_kv\", mount = \"m\", path = \"p\", key = \"k\" }";
        let reference = parsed(written).expect("a reference");
        let dumped = toml::to_string(&Dumped { secret: reference }).expect("serialisable");
        let reread = parsed(&dumped).expect("a dumped reference reads back");
        assert!(reread.is_reference(), "{dumped}");
    }

    #[test]
    fn an_empty_string_is_a_blank_value() {
        assert!(parsed("secret = \"\"").expect("blank").is_blank_value());
        assert!(!parsed("secret = \"x\"").expect("a value").is_blank_value());
    }
}
