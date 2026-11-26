//! [`NkeySigner`]: where the seed behind a NATS NKey lives, and signs.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::settings::{is_transit_key_name, is_transit_mount};

/// What signs the server's nonce for an NKey connection; the seed never enters the process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Written", into = "Written")]
// Exhaustive on purpose, as `NatsAuth` is.
pub enum NkeySigner {
    /// An Ed25519 key a Vault or OpenBao Transit engine created, non-exportable.
    VaultTransit {
        /// The Transit engine's mount, e.g. `transit`.
        mount: String,
        /// The key's name under the mount.
        key: String,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
enum Written {
    VaultTransit { mount: String, key: String },
}

impl TryFrom<Written> for NkeySigner {
    type Error = String;

    fn try_from(written: Written) -> Result<Self, Self::Error> {
        let Written::VaultTransit { mount, key } = written;
        let mut refused = Vec::new();
        if !is_transit_mount(&mount) {
            refused.push("mount is 1-64 characters of [a-z0-9-]");
        }
        if !is_transit_key_name(&key) {
            refused.push("key is 1-128 characters of [a-z0-9_-]");
        }
        if refused.is_empty() {
            Ok(Self::VaultTransit { mount, key })
        } else {
            Err(format!(
                "a vault_transit signer's {}",
                refused.join("; its ")
            ))
        }
    }
}

impl From<NkeySigner> for Written {
    fn from(signer: NkeySigner) -> Self {
        match signer {
            NkeySigner::VaultTransit { mount, key } => Self::VaultTransit { mount, key },
        }
    }
}

impl fmt::Display for NkeySigner {
    /// `vault_transit <mount>/<key>`: which key signs, never anything it signed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VaultTransit { mount, key } => write!(f, "vault_transit {mount}/{key}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NkeySigner;

    #[derive(Debug, serde::Deserialize)]
    struct Holder {
        signer: NkeySigner,
    }

    fn parsed(toml: &str) -> Result<NkeySigner, toml::de::Error> {
        toml::from_str::<Holder>(toml).map(|holder| holder.signer)
    }

    #[test]
    fn a_transit_signer_is_read() {
        let signer = parsed(
            "signer = { source = \"vault_transit\", mount = \"transit\", key = \"nats-app\" }",
        )
        .expect("a signer");
        assert_eq!(
            signer,
            NkeySigner::VaultTransit {
                mount: "transit".into(),
                key: "nats-app".into()
            }
        );
        assert_eq!(signer.to_string(), "vault_transit transit/nats-app");
    }

    #[test]
    fn a_malformed_signer_is_refused_for_what_is_wrong_with_it() {
        for (toml, why) in [
            (
                "signer = { source = \"vault_kv\", mount = \"m\", key = \"k\" }",
                "unknown variant `vault_kv`",
            ),
            (
                "signer = { source = \"vault_transit\", mount = \"m\", key = \"k\", seed = \"S\" }",
                "unknown field `seed`",
            ),
            (
                "signer = { source = \"vault_transit\", mount = \"m\", key = \"\" }",
                "key is 1-128 characters",
            ),
            (
                "signer = { source = \"vault_transit\", mount = \"Transit_1\", key = \"k\" }",
                "mount is 1-64 characters",
            ),
        ] {
            let refused = parsed(toml).expect_err(toml);
            assert!(refused.to_string().contains(why), "{toml}: {refused}");
        }
    }
}
