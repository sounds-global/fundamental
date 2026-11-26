//! [`VaultKvRef`]: where a key-value secret store holds one secret.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::is_secret_path;

/// Where a KV v2 secret store holds one secret: mount, path and field; unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Written", into = "Written")]
pub struct VaultKvRef {
    /// The key-value engine's mount, e.g. `secret`.
    pub mount: String,
    /// The secret's path under the mount, e.g. `nats/orders`.
    pub path: String,
    /// The field of the secret that holds the value, e.g. `password`.
    pub key: String,
}

impl fmt::Display for VaultKvRef {
    /// `vault_kv <mount>/<path>#<key>`: where the secret is, never what it is.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vault_kv {}/{}#{}", self.mount, self.path, self.key)
    }
}

/// A one-variant enum: serde honours `deny_unknown_fields` beside an internal tag only on an enum.
#[derive(Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
enum Written {
    VaultKv {
        mount: String,
        path: String,
        key: String,
    },
}

impl TryFrom<Written> for VaultKvRef {
    type Error = String;

    /// Refuses a mount or path that is not [`is_secret_path`], or a key empty or holding a control character.
    fn try_from(written: Written) -> Result<Self, Self::Error> {
        let Written::VaultKv { mount, path, key } = written;
        let path = path.trim_matches('/').to_owned();
        let mut refused = Vec::new();
        if !is_secret_path(&mount) {
            refused.push("mount");
        }
        if !is_secret_path(&path) {
            refused.push("path");
        }
        if key.is_empty() || key.chars().any(char::is_control) {
            refused.push("key");
        }
        if refused.is_empty() {
            Ok(Self { mount, path, key })
        } else {
            Err(format!(
                "a vault_kv reference names one secret — a mount and path of plain `/`-separated \
                 segments, and a key; its {} {} not",
                refused.join(" and "),
                if refused.len() == 1 { "is" } else { "are" }
            ))
        }
    }
}

impl From<VaultKvRef> for Written {
    fn from(reference: VaultKvRef) -> Self {
        Self::VaultKv {
            mount: reference.mount,
            path: reference.path,
            key: reference.key,
        }
    }
}
