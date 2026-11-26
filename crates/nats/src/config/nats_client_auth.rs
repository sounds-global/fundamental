use std::path::PathBuf;

use fundamental_core::secrets::SecretString;
use fundamental_secrets::RefreshedSecret;
use serde::{Deserialize, Serialize};

use super::NkeyTransitSigner;

/// How a [`NatsClient`](crate::NatsClient) authenticates: one credential, or none.
///
/// One field, so a connection presents exactly one credential. The last three variants are
/// live and exist only after [`NatsConfig::from_settings`](super::NatsConfig::from_settings);
/// serialising one is an error, so a configuration never round-trips without its credential.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
// Exhaustive: a new method must fail to compile where the connection is built.
pub enum NatsClientAuth {
    /// No credential: an open server, or one that authenticates by TLS client certificate.
    #[default]
    None,
    /// A username and a password given as a value.
    UserPassword {
        /// The username.
        username: String,
        /// The password.
        password: SecretString,
    },
    /// A bearer token given as a value.
    Token {
        /// The token.
        token: SecretString,
    },
    /// A `.creds` file holding a user JWT and its NKey seed.
    CredentialsFile {
        /// Where the file is.
        path: PathBuf,
    },
    /// A username and a password the secret store holds, answered from the value last read.
    #[serde(skip)]
    StoredUserPassword {
        /// The username.
        username: String,
        /// The password, kept current in the background.
        password: RefreshedSecret,
    },
    /// A bearer token the secret store holds, answered from the value last read.
    #[serde(skip)]
    StoredToken {
        /// The token, kept current in the background.
        token: RefreshedSecret,
    },
    /// A user NKey whose seed is a Transit key; each connection attempt signs the nonce there.
    #[serde(skip)]
    NKey(NkeyTransitSigner),
}

impl NatsClientAuth {
    /// Whether this credential crosses the connection as itself rather than as a signature.
    #[must_use]
    pub fn sends_a_secret(&self) -> bool {
        match self {
            Self::UserPassword { .. }
            | Self::Token { .. }
            | Self::StoredUserPassword { .. }
            | Self::StoredToken { .. } => true,
            Self::None | Self::CredentialsFile { .. } | Self::NKey(_) => false,
        }
    }
}
