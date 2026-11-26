//! [`NatsAuth`]: how a NATS client proves who it is.

use serde::Serialize;

use super::{NkeyPublicKey, NkeySigner};
use crate::ConfigError;
use crate::settings::SecretRef;

/// How a NATS client authenticates: one method, or none; a half pair cannot be represented.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "method", rename_all = "snake_case")]
// Exhaustive on purpose: a new method must fail to compile rather than connect as nobody.
pub enum NatsAuth {
    /// No credential: an open server, or one that authenticates by TLS client certificate.
    #[default]
    None,
    /// A username and its password.
    UserPassword {
        /// The username.
        username: String,
        /// The password, or where the secret store holds it.
        password: SecretRef,
    },
    /// A bearer token.
    Token {
        /// The token, or where the secret store holds it.
        token: SecretRef,
    },
    /// A `.creds` file holding a user JWT and its NKey seed.
    CredentialsFile {
        /// Where the file is.
        path: String,
    },
    /// A user NKey whose seed never leaves its signer, which signs each server nonce.
    #[serde(rename = "nkey")]
    NKey {
        /// The user's public key, as the server's configuration names it.
        public_key: NkeyPublicKey,
        /// What holds the seed and signs.
        signer: NkeySigner,
    },
}

impl NatsAuth {
    /// The one method the flat keys configure; a blank value is an absent one.
    ///
    /// # Errors
    /// [`ConfigError::InvalidValue`] naming `messaging.nats` for a half pair or more than one method.
    pub fn from_keys(
        username: Option<String>,
        password: Option<SecretRef>,
        token: Option<SecretRef>,
        credentials_path: Option<String>,
        nkey: Option<(NkeyPublicKey, NkeySigner)>,
    ) -> Result<Self, ConfigError> {
        let username = username.filter(|value| !value.is_empty());
        let password = password.filter(|secret| !secret.is_blank_value());
        let token = token.filter(|secret| !secret.is_blank_value());
        let credentials_path = credentials_path.filter(|value| !value.is_empty());

        let mut methods = Vec::new();
        let user_password = match (username, password) {
            (Some(username), Some(password)) => {
                methods.push("username/password");
                Some(Self::UserPassword { username, password })
            }
            (Some(_), None) => return Err(refused("a username is set without its password")),
            (None, Some(_)) => return Err(refused("a password is set without a username")),
            (None, None) => None,
        };
        let token = token.map(|token| {
            methods.push("token");
            Self::Token { token }
        });
        let file = credentials_path.map(|path| {
            methods.push("credentials_path");
            Self::CredentialsFile { path }
        });
        let nkey = nkey.map(|(public_key, signer)| {
            methods.push("nkey");
            Self::NKey { public_key, signer }
        });
        if methods.len() > 1 {
            return Err(refused(&format!(
                "{} are all set; configure one authentication method",
                methods.join(", ")
            )));
        }
        Ok(user_password
            .or(token)
            .or(file)
            .or(nkey)
            .unwrap_or_default())
    }

    /// The secret this method carries as a plain value, if any, named by its key.
    #[must_use]
    pub fn plain_secret_key(&self) -> Option<&'static str> {
        match self {
            Self::UserPassword { password, .. } if !password.is_reference() => Some("password"),
            Self::Token { token } if !token.is_reference() => Some("token"),
            Self::UserPassword { .. }
            | Self::Token { .. }
            | Self::None
            | Self::CredentialsFile { .. }
            | Self::NKey { .. } => None,
        }
    }

    /// Whether connecting needs the secret store: a reference to read, or a signer to sign.
    #[must_use]
    pub fn requires_secret_manager(&self) -> bool {
        match self {
            Self::UserPassword {
                password: secret, ..
            }
            | Self::Token { token: secret } => secret.is_reference(),
            Self::NKey { .. } => true,
            Self::None | Self::CredentialsFile { .. } => false,
        }
    }

    /// Whether this method sends a password or token over the connection itself.
    #[must_use]
    pub fn sends_a_secret(&self) -> bool {
        matches!(self, Self::UserPassword { .. } | Self::Token { .. })
    }
}

fn refused(reason: &str) -> ConfigError {
    ConfigError::invalid_value("messaging.nats", reason)
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;
    use fundamental_core::secrets::SecretString;

    use super::NatsAuth;
    use crate::settings::{NkeyPublicKey, NkeySigner, SecretRef, VaultKvRef};

    fn value(secret: &str) -> Option<SecretRef> {
        Some(SecretRef::Value(SecretString::new(secret.to_owned())))
    }

    fn reference() -> Option<SecretRef> {
        Some(SecretRef::VaultKv(VaultKvRef {
            mount: "secret".into(),
            path: "nats/app".into(),
            key: "password".into(),
        }))
    }

    fn nkey() -> Option<(NkeyPublicKey, NkeySigner)> {
        Some((
            NkeyPublicKey::try_from(
                "UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP".to_owned(),
            )
            .expect("a user key"),
            NkeySigner::VaultTransit {
                mount: "transit".into(),
                key: "nats-app".into(),
            },
        ))
    }

    #[test]
    fn a_username_and_its_password_are_one_method() {
        let auth = NatsAuth::from_keys(Some("app".into()), value("pw"), None, None, None)
            .expect("a whole pair");
        let NatsAuth::UserPassword { username, password } = &auth else {
            panic!("expected a username and password, got {auth:?}");
        };
        assert_eq!(username, "app");
        let SecretRef::Value(password) = password else {
            panic!("a written password is a value");
        };
        assert_eq!(password.expose(), "pw");
        assert_eq!(auth.plain_secret_key(), Some("password"));
        assert!(!auth.requires_secret_manager());
        assert!(auth.sends_a_secret());
    }

    #[test]
    fn a_referenced_password_is_no_plain_secret_and_needs_the_store() {
        let auth = NatsAuth::from_keys(Some("app".into()), reference(), None, None, None)
            .expect("a whole pair");
        assert_eq!(auth.plain_secret_key(), None);
        assert!(auth.requires_secret_manager());
        assert!(auth.sends_a_secret());
    }

    #[test]
    fn an_nkey_is_its_own_method_and_sends_no_secret() {
        let auth = NatsAuth::from_keys(None, None, None, None, nkey()).expect("an nkey");
        assert!(matches!(auth, NatsAuth::NKey { .. }));
        assert_eq!(auth.plain_secret_key(), None);
        assert!(auth.requires_secret_manager());
        assert!(!auth.sends_a_secret());
    }

    #[test]
    fn a_half_pair_is_refused_either_way_round() {
        for (username, password, reason) in [
            (Some("app".to_owned()), None, "without its password"),
            (None, value("pw"), "without a username"),
            (None, reference(), "without a username"),
        ] {
            let refused = NatsAuth::from_keys(username, password, None, None, None)
                .expect_err("half a pair authenticates as nobody");
            assert_eq!(refused.error_code(), "CFG_004");
            assert!(refused.to_string().contains(reason), "{refused}");
        }
    }

    #[test]
    fn two_methods_at_once_are_refused() {
        let refused = NatsAuth::from_keys(
            Some("app".into()),
            value("pw"),
            value("tok"),
            Some("/run/app.creds".into()),
            nkey(),
        )
        .expect_err("which one would it be");
        assert!(
            refused
                .to_string()
                .contains("username/password, token, credentials_path, nkey"),
            "{refused}"
        );
    }

    #[test]
    fn an_empty_value_configures_nothing() {
        let auth = NatsAuth::from_keys(
            Some(String::new()),
            value(""),
            value(""),
            Some(String::new()),
            None,
        )
        .expect("nothing configured");
        assert!(matches!(auth, NatsAuth::None));
    }

    #[test]
    fn a_token_or_a_credentials_file_alone_is_its_method() {
        assert!(matches!(
            NatsAuth::from_keys(None, None, value("tok"), None, None),
            Ok(NatsAuth::Token { .. })
        ));
        assert!(matches!(
            NatsAuth::from_keys(None, None, None, Some("/run/app.creds".into()), None),
            Ok(NatsAuth::CredentialsFile { path }) if path == "/run/app.creds"
        ));
    }
}
