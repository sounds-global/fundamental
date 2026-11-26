//! NATS messaging settings ([`NatsSettings`]).

use fundamental_core::Environment;
use serde::{Deserialize, Serialize};

use super::{NatsAuth, NkeyPublicKey, NkeySigner};
use crate::ConfigError;
use crate::settings::{SecretRef, names_loopback};

const DEFAULT_SECRET_REFRESH_SECONDS: u64 = 300;

/// At most one read a second per secret, and at least one a day, so a rotation always arrives.
const SECRET_REFRESH_SECONDS: std::ops::RangeInclusive<u64> = 1..=86_400;

/// NATS configuration; an unknown key is refused, since the likeliest misspelling is a credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RawNatsSettings")]
pub struct NatsSettings {
    /// NATS server URL.
    pub url: String,
    /// Whether TLS is enabled.
    pub tls_enabled: bool,
    /// Path to the CA certificate (PEM) that signs the server's certificate.
    pub tls_ca_cert_path: Option<String>,
    /// Path to this client's certificate (PEM), for mutual TLS.
    pub tls_client_cert_path: Option<String>,
    /// Path to this client's private key (PEM), for mutual TLS.
    pub tls_client_key_path: Option<String>,
    /// How the client authenticates.
    pub auth: NatsAuth,
    /// Connection name.
    pub connection_name: Option<String>,
    /// The prefix this client's reply inboxes are drawn under, instead of the shared `_INBOX`.
    pub inbox_prefix: Option<String>,
    /// How often, in seconds, a secret-store password or token is read again (1 to 86 400; 300).
    pub secret_refresh_seconds: u64,
    /// Credential keys written but resolved to nothing; refused when deployed.
    #[serde(skip)]
    pub blank_credentials: Vec<&'static str>,
}

impl NatsSettings {
    /// Whether this connection uses TLS: `tls_enabled`, or a `tls://` server URL.
    #[must_use]
    pub fn requests_tls(&self) -> bool {
        self.tls_enabled || requests_tls_by_url(&self.url)
    }

    /// Whether connecting needs the secret store.
    #[must_use]
    pub fn requires_secret_manager(&self) -> bool {
        self.auth.requires_secret_manager()
    }

    /// Refuses, deployed, a NATS credential left blank, written as a value, or sent in the clear off-host.
    ///
    /// # Errors
    /// [`ConfigError::InvalidValue`] naming `messaging.nats` and what was refused.
    pub fn require_deployed_credentials(
        &self,
        environment: Environment,
    ) -> Result<(), ConfigError> {
        if !environment.is_deployed() {
            return Ok(());
        }
        if !self.blank_credentials.is_empty() {
            return Err(ConfigError::invalid_value(
                "messaging.nats",
                format!(
                    "{} resolved to nothing; {environment} connects only with the credential it \
                     was configured with",
                    self.blank_credentials.join(", ")
                ),
            ));
        }
        if let Some(key) = self.auth.plain_secret_key() {
            return Err(ConfigError::invalid_value(
                "messaging.nats",
                format!(
                    "{key} is a plain value; {environment} reads NATS secrets only from the secret \
                     store — declare `{key} = {{ source = \"vault_kv\", mount, path, key }}`, or \
                     authenticate with `nkey` or `credentials_path`"
                ),
            ));
        }
        if !matches!(self.auth, NatsAuth::None)
            && !self.requests_tls()
            && !self.url.split(',').all(names_loopback)
        {
            return Err(ConfigError::invalid_value(
                "messaging.nats",
                format!(
                    "the credential would cross the network in the clear; {environment} \
                     presents one only over TLS — set `tls_enabled = true` or a `tls://` url"
                ),
            ));
        }
        Ok(())
    }
}

impl Default for NatsSettings {
    fn default() -> Self {
        Self {
            url: "nats://localhost:4222".to_string(),
            tls_enabled: false,
            tls_ca_cert_path: None,
            tls_client_cert_path: None,
            tls_client_key_path: None,
            auth: NatsAuth::None,
            connection_name: None,
            inbox_prefix: None,
            secret_refresh_seconds: DEFAULT_SECRET_REFRESH_SECONDS,
            blank_credentials: Vec::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawNatsSettings {
    url: String,
    tls_enabled: bool,
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    tls_ca_cert_path: Option<String>,
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    tls_client_cert_path: Option<String>,
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    tls_client_key_path: Option<String>,
    username: Option<String>,
    password: Option<SecretRef>,
    token: Option<SecretRef>,
    credentials_path: Option<String>,
    nkey: Option<RawNkey>,
    connection_name: Option<String>,
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    inbox_prefix: Option<String>,
    secret_refresh_seconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNkey {
    public_key: NkeyPublicKey,
    signer: NkeySigner,
}

impl Default for RawNatsSettings {
    fn default() -> Self {
        let defaults = NatsSettings::default();
        Self {
            url: defaults.url,
            tls_enabled: defaults.tls_enabled,
            tls_ca_cert_path: None,
            tls_client_cert_path: None,
            tls_client_key_path: None,
            username: None,
            password: None,
            token: None,
            credentials_path: None,
            nkey: None,
            connection_name: None,
            inbox_prefix: None,
            secret_refresh_seconds: defaults.secret_refresh_seconds,
        }
    }
}

impl TryFrom<RawNatsSettings> for NatsSettings {
    type Error = ConfigError;

    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn try_from(raw: RawNatsSettings) -> Result<Self, Self::Error> {
        let tls_material = [
            ("tls_ca_cert_path", &raw.tls_ca_cert_path),
            ("tls_client_cert_path", &raw.tls_client_cert_path),
            ("tls_client_key_path", &raw.tls_client_key_path),
        ];
        let configured: Vec<&str> = tls_material
            .iter()
            .filter(|(_, path)| path.is_some())
            .map(|(key, _)| *key)
            .collect();
        if !configured.is_empty() && !raw.tls_enabled && !requests_tls_by_url(&raw.url) {
            return Err(ConfigError::invalid_value(
                "messaging.nats",
                format!(
                    "{} set while TLS is off; set `tls_enabled = true` or a `tls://` url, or \
                     the connection is made without them",
                    configured.join(", ")
                ),
            ));
        }
        if raw.tls_client_cert_path.is_some() != raw.tls_client_key_path.is_some() {
            return Err(ConfigError::invalid_value(
                "messaging.nats",
                "a TLS client certificate and its key are set together or not at all",
            ));
        }
        let blank_credentials = [
            (
                "username",
                raw.username.as_deref().is_some_and(str::is_empty),
            ),
            (
                "password",
                raw.password.as_ref().is_some_and(SecretRef::is_blank_value),
            ),
            (
                "token",
                raw.token.as_ref().is_some_and(SecretRef::is_blank_value),
            ),
            (
                "credentials_path",
                raw.credentials_path.as_deref().is_some_and(str::is_empty),
            ),
        ]
        .into_iter()
        .filter_map(|(key, blank)| blank.then_some(key))
        .collect();
        if !SECRET_REFRESH_SECONDS.contains(&raw.secret_refresh_seconds) {
            return Err(ConfigError::invalid_value(
                "messaging.nats",
                format!(
                    "secret_refresh_seconds is {}; it is between {} and {}",
                    raw.secret_refresh_seconds,
                    SECRET_REFRESH_SECONDS.start(),
                    SECRET_REFRESH_SECONDS.end()
                ),
            ));
        }
        Ok(Self {
            auth: NatsAuth::from_keys(
                raw.username,
                raw.password,
                raw.token,
                raw.credentials_path,
                raw.nkey.map(|nkey| (nkey.public_key, nkey.signer)),
            )?,
            secret_refresh_seconds: raw.secret_refresh_seconds,
            blank_credentials,
            url: raw.url,
            tls_enabled: raw.tls_enabled,
            tls_ca_cert_path: raw.tls_ca_cert_path,
            tls_client_cert_path: raw.tls_client_cert_path,
            tls_client_key_path: raw.tls_client_key_path,
            connection_name: raw.connection_name,
            inbox_prefix: raw.inbox_prefix,
        })
    }
}

fn requests_tls_by_url(url: &str) -> bool {
    url.split(',').any(|url| url.trim().starts_with("tls://"))
}
