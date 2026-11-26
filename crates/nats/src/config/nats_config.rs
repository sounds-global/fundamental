use std::path::PathBuf;
use std::time::Duration;

use fundamental_config::settings::{NatsAuth, NatsSettings, SecretRef};
use fundamental_core::Environment;
use fundamental_core::secrets::Secret;
use fundamental_secrets::{RefreshedSecret, VaultClient};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

use super::{NatsClientAuth, NatsTlsConfig, NkeyTransitSigner, ReconnectConfig};
use crate::StoreAnswer;
use crate::error::{NatsError, NatsResult};
use crate::subject::SubjectPrefix;

/// Configuration for connecting a NATS client.
///
/// The credential is one field, [`auth`](Self::auth); store-held ones are resolved by
/// [`from_settings`](Self::from_settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsConfig {
    /// Server URL, or a comma-separated list of URLs.
    pub url: String,
    /// Client name reported to the server.
    #[serde(default)]
    pub name: Option<String>,
    /// How the connection authenticates.
    #[serde(default)]
    pub auth: NatsClientAuth,
    /// TLS settings.
    #[serde(default)]
    pub tls: Option<NatsTlsConfig>,
    /// Reconnection policy.
    #[serde(default)]
    pub reconnect: ReconnectConfig,
    /// Connection timeout.
    #[serde(default = "default_connect_timeout", with = "super::duration_serde")]
    pub connect_timeout: Duration,
    /// Timeout for request-reply calls.
    #[serde(default = "default_request_timeout", with = "super::duration_serde")]
    pub request_timeout: Duration,
    /// Maximum payload size in bytes.
    #[serde(default = "default_max_payload")]
    pub max_payload: usize,
    /// Keepalive ping interval.
    #[serde(default = "default_ping_interval", with = "super::duration_serde")]
    pub ping_interval: Duration,
    /// Prefix for reply inboxes.
    #[serde(default = "default_inbox_prefix")]
    pub inbox_prefix: String,
    /// The namespace every subject this client names is relative to, if any.
    ///
    /// `None` by default: subjects reach the wire as the message types spell them.
    #[serde(default)]
    pub subject_prefix: Option<SubjectPrefix>,
    /// Whether the server echoes the client's own messages back.
    #[serde(default)]
    pub echo: bool,
    /// Per-subscription buffer capacity.
    #[serde(default = "default_subscription_capacity")]
    pub subscription_capacity: usize,
}

fn default_connect_timeout() -> Duration {
    Duration::from_secs(5)
}

fn default_request_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_max_payload() -> usize {
    1024 * 1024
}

fn default_ping_interval() -> Duration {
    Duration::from_secs(60)
}

fn default_inbox_prefix() -> String {
    "_INBOX".to_string()
}

fn default_subscription_capacity() -> usize {
    4096
}

impl NatsConfig {
    /// Creates a configuration for `url` with default timeouts and reconnection.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            name: None,
            auth: NatsClientAuth::None,
            tls: None,
            reconnect: ReconnectConfig::default(),
            connect_timeout: default_connect_timeout(),
            request_timeout: default_request_timeout(),
            max_payload: default_max_payload(),
            ping_interval: default_ping_interval(),
            inbox_prefix: default_inbox_prefix(),
            subject_prefix: None,
            echo: false,
            subscription_capacity: default_subscription_capacity(),
        }
    }

    /// Sets the client name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the prefix this client's reply inboxes are drawn under.
    ///
    /// A per-service prefix lets a broker grant `_INBOX.<service>.>` rather than `_INBOX.>`.
    #[must_use]
    pub fn with_inbox_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.inbox_prefix = prefix.into();
        self
    }

    /// Sets the namespace every subject this client names is relative to.
    ///
    /// Publishes, subscriptions, requests and JetStream stream subjects move with it; reply
    /// inboxes do not. A namespace, not a permission boundary: that is a NATS account.
    ///
    /// # Errors
    /// Returns [`NatsError::InvalidSubject`](crate::NatsError) if `prefix` is not a valid
    /// wildcard-free subject.
    pub fn with_subject_prefix(mut self, prefix: impl Into<String>) -> NatsResult<Self> {
        self.subject_prefix = Some(SubjectPrefix::new(prefix)?);
        Ok(self)
    }

    /// Authenticates with a bearer token, replacing any other credential.
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.auth = NatsClientAuth::Token {
            token: Secret::new(token.into()),
        };
        self
    }

    /// Authenticates with a username and password, replacing any other credential.
    #[must_use]
    pub fn with_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.auth = NatsClientAuth::UserPassword {
            username: username.into(),
            password: Secret::new(password.into()),
        };
        self
    }

    /// Authenticates with a `.creds` file, replacing any other credential.
    #[must_use]
    pub fn with_credentials_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.auth = NatsClientAuth::CredentialsFile { path: path.into() };
        self
    }

    /// Authenticates with the credential `auth` describes, replacing any other.
    #[must_use]
    pub fn with_auth(mut self, auth: NatsClientAuth) -> Self {
        self.auth = auth;
        self
    }

    /// Sets TLS settings.
    #[must_use]
    pub fn with_tls(mut self, tls: NatsTlsConfig) -> Self {
        self.tls = Some(tls);
        self
    }

    /// Sets the reconnection policy.
    #[must_use]
    pub fn with_reconnect(mut self, reconnect: ReconnectConfig) -> Self {
        self.reconnect = reconnect;
        self
    }

    /// Sets the request-reply timeout.
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets the connection timeout.
    #[must_use]
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Returns the configured URLs, splitting a comma-separated list.
    #[must_use]
    pub fn urls(&self) -> Vec<&str> {
        self.url.split(',').map(str::trim).collect()
    }

    /// Builds a configuration from loaded [`NatsSettings`], resolving its credential.
    ///
    /// A store-held password or token is read now through `secrets` and refreshed every
    /// `secret_refresh_seconds`; an NKey is checked against its Transit key now. `secrets` may be
    /// `None` when nothing needs the store.
    ///
    /// # Errors
    /// - [`NatsError::SecretUnreadable`] (`NATS_023`) when a referenced password or token
    ///   cannot be read, or no `secrets` was given to read it.
    /// - [`NatsError::NkeyUnusable`] (`NATS_024`) and [`NatsError::NkeySignerFailed`]
    ///   (`NATS_025`) from [`NkeyTransitSigner::connect`].
    #[instrument(skip_all, fields(nats.url = %settings.url), err(Debug))]
    pub async fn from_settings(
        settings: &NatsSettings,
        _environment: Environment,
        secrets: Option<&VaultClient>,
    ) -> NatsResult<Self> {
        let mut config = Self::new(&settings.url);
        if let Some(name) = &settings.connection_name {
            config = config.with_name(name);
        }
        if let Some(prefix) = &settings.inbox_prefix {
            config = config.with_inbox_prefix(prefix);
        }
        let refresh = Duration::from_secs(settings.secret_refresh_seconds);
        config.auth = match &settings.auth {
            NatsAuth::None => NatsClientAuth::None,
            NatsAuth::UserPassword {
                username,
                password: SecretRef::Value(password),
            } => NatsClientAuth::UserPassword {
                username: username.clone(),
                password: password.clone(),
            },
            NatsAuth::UserPassword {
                username,
                password: SecretRef::VaultKv(reference),
            } => NatsClientAuth::StoredUserPassword {
                username: username.clone(),
                password: stored(secrets, reference, refresh).await?,
            },
            NatsAuth::Token {
                token: SecretRef::Value(token),
            } => NatsClientAuth::Token {
                token: token.clone(),
            },
            NatsAuth::Token {
                token: SecretRef::VaultKv(reference),
            } => NatsClientAuth::StoredToken {
                token: stored(secrets, reference, refresh).await?,
            },
            NatsAuth::CredentialsFile { path } => NatsClientAuth::CredentialsFile {
                path: PathBuf::from(path),
            },
            NatsAuth::NKey { public_key, signer } => {
                NatsClientAuth::NKey(NkeyTransitSigner::connect(secrets, signer, public_key).await?)
            }
        };
        // A `tls://` URL turns TLS on by itself, as the driver does; a boolean cannot come from
        // `${VAR}` interpolation, so the scheme is the reachable switch.
        if settings.requests_tls() {
            // The anchor and client identity come from settings, both halves or neither: the
            // platform roots alone cannot reach a privately signed fabric.
            let mut tls = NatsTlsConfig::new();
            if let Some(ca_cert) = settings.tls_ca_cert_path.as_deref() {
                tls = tls.with_ca_cert(ca_cert);
            }
            if let (Some(cert), Some(key)) = (
                settings.tls_client_cert_path.as_deref(),
                settings.tls_client_key_path.as_deref(),
            ) {
                tls = tls.with_client_cert(cert, key);
            }
            config = config.with_tls(tls);
        }
        Ok(config)
    }
}

async fn stored(
    secrets: Option<&VaultClient>,
    reference: &fundamental_config::settings::VaultKvRef,
    refresh: Duration,
) -> NatsResult<RefreshedSecret> {
    let source_ref = reference.to_string();
    let Some(secrets) = secrets else {
        return Err(NatsError::SecretUnreadable {
            source_ref,
            answer: StoreAnswer::Refused,
            cause: fundamental_secrets::SecretError::NotConfigured(
                "no secret store was given to NatsConfig::from_settings".to_owned(),
            ),
        });
    };
    let secret = secrets
        .refreshed_secret(reference, refresh)
        .await
        .map_err(|cause| NatsError::SecretUnreadable {
            source_ref: source_ref.clone(),
            answer: StoreAnswer::of(&cause),
            cause,
        })?;
    info!(
        secret.source = %source_ref,
        refresh_seconds = refresh.as_secs(),
        "NATS credential read from the secret store"
    );
    Ok(secret)
}

impl Default for NatsConfig {
    fn default() -> Self {
        Self::new("nats://localhost:4222")
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn an_inbox_prefix_from_settings_reaches_the_config() {
        let settings = fundamental_config::settings::NatsSettings {
            inbox_prefix: Some("_INBOX.orders".to_string()),
            ..Default::default()
        };
        let config = NatsConfig::from_settings(&settings, Environment::Development, None)
            .await
            .expect("resolves");
        assert_eq!(config.inbox_prefix, "_INBOX.orders");
    }

    #[tokio::test]
    async fn an_absent_inbox_prefix_keeps_the_shared_default() {
        let settings = fundamental_config::settings::NatsSettings::default();
        let config = NatsConfig::from_settings(&settings, Environment::Development, None)
            .await
            .expect("resolves");
        assert_eq!(
            config.inbox_prefix, "_INBOX",
            "an unset prefix must not change behaviour for a deployment that never asked"
        );
    }

    use super::*;
    use fundamental_config::settings::VaultKvRef;
    use fundamental_core::Error as _;
    use fundamental_core::secrets::SecretString;
    use std::path::Path;

    /// A reference with no secret store is refused, never connected without it.
    #[tokio::test]
    async fn a_reference_with_no_secret_store_is_refused_naming_where_it_is() {
        let settings = NatsSettings {
            auth: NatsAuth::Token {
                token: SecretRef::VaultKv(VaultKvRef {
                    mount: "secret".into(),
                    path: "nats/app".into(),
                    key: "token".into(),
                }),
            },
            ..Default::default()
        };
        let refused = NatsConfig::from_settings(&settings, Environment::Production, None)
            .await
            .expect_err("nothing can read the reference");
        assert!(
            matches!(refused, NatsError::SecretUnreadable { .. }),
            "{refused:?}"
        );
        assert_eq!(refused.error_code(), "NATS_023");
        assert!(!refused.is_retryable());
        assert!(
            refused
                .to_string()
                .contains("vault_kv secret/nats/app#token"),
            "{refused}"
        );
    }

    #[test]
    fn new_sets_url_and_defaults() {
        let cfg = NatsConfig::new("nats://localhost:4222");
        assert_eq!(cfg.url, "nats://localhost:4222");
        assert!(cfg.name.is_none());
        assert_eq!(cfg.max_payload, 1024 * 1024);
        assert!(cfg.reconnect.retry_initial_connect);
    }

    #[test]
    fn urls_splits_and_trims() {
        let cfg = NatsConfig::new("nats://h1:4222, nats://h2:4222");
        assert_eq!(cfg.urls(), ["nats://h1:4222", "nats://h2:4222"]);
    }

    #[test]
    fn secrets_are_stored_and_redacted() {
        let token = NatsConfig::new("nats://localhost:4222").with_token("tok-123");
        let NatsClientAuth::Token { token: secret } = &token.auth else {
            panic!("a token: {:?}", token.auth);
        };
        assert_eq!(secret.expose(), "tok-123");

        let cfg = token.clone().with_credentials("user", "pass-456");
        let NatsClientAuth::UserPassword { username, password } = &cfg.auth else {
            panic!("the later credential replaces the earlier: {:?}", cfg.auth);
        };
        assert_eq!(username, "user");
        assert_eq!(password.expose(), "pass-456");

        let debug = format!("{cfg:?}{token:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("tok-123"));
        assert!(!debug.contains("pass-456"));
    }

    #[test]
    fn serialize_redacts_secrets() {
        let cfg = NatsConfig::new("nats://localhost:4222").with_token("tok-123");
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(!json.contains("tok-123"));
        assert!(json.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn from_settings_maps_fields() {
        let settings = NatsSettings {
            url: "nats://custom:4222".to_string(),
            tls_enabled: true,
            auth: NatsAuth::UserPassword {
                username: "u".to_string(),
                password: SecretRef::Value(SecretString::new("p".to_string())),
            },
            connection_name: Some("svc".to_string()),
            ..Default::default()
        };
        let cfg = NatsConfig::from_settings(&settings, Environment::Production, None)
            .await
            .expect("resolves");
        assert_eq!(cfg.url, "nats://custom:4222");
        assert_eq!(cfg.name.as_deref(), Some("svc"));
        assert!(matches!(
            &cfg.auth,
            NatsClientAuth::UserPassword { username, password }
                if username == "u" && password.expose() == "p"
        ));
        assert!(cfg.tls.is_some());
    }

    #[tokio::test]
    async fn a_token_or_a_credentials_file_from_settings_reaches_the_config() {
        let token = NatsSettings {
            auth: NatsAuth::Token {
                token: SecretRef::Value(SecretString::new("tok".to_string())),
            },
            ..Default::default()
        };
        let cfg = NatsConfig::from_settings(&token, Environment::Production, None)
            .await
            .expect("resolves");
        assert!(matches!(&cfg.auth, NatsClientAuth::Token { token } if token.expose() == "tok"));

        let file = NatsSettings {
            auth: NatsAuth::CredentialsFile {
                path: "/run/app.creds".to_string(),
            },
            ..Default::default()
        };
        let cfg = NatsConfig::from_settings(&file, Environment::Production, None)
            .await
            .expect("resolves");
        assert!(matches!(
            &cfg.auth,
            NatsClientAuth::CredentialsFile { path } if path == Path::new("/run/app.creds")
        ));
    }

    /// The anchor and client identity survive the settings mapping.
    #[tokio::test]
    async fn from_settings_carries_the_trust_anchor_and_client_identity() {
        let settings = NatsSettings {
            url: "nats://nats:4222".to_string(),
            tls_enabled: true,
            tls_ca_cert_path: Some("/certs/ca.crt".to_string()),
            tls_client_cert_path: Some("/certs/client.layer4.crt".to_string()),
            tls_client_key_path: Some("/certs/client.layer4.key".to_string()),
            ..Default::default()
        };

        let tls = NatsConfig::from_settings(&settings, Environment::Production, None)
            .await
            .expect("resolves")
            .tls
            .expect("tls_enabled builds a TLS configuration");

        assert_eq!(tls.ca_cert.as_deref(), Some(Path::new("/certs/ca.crt")));
        assert_eq!(
            tls.client_key.as_deref(),
            Some(Path::new("/certs/client.layer4.key"))
        );
    }

    /// A `tls://` URL turns TLS on.
    #[tokio::test]
    async fn a_tls_url_configures_tls_without_the_flag() {
        let settings = NatsSettings {
            url: "tls://nats:4222".to_string(),
            tls_enabled: false,
            tls_ca_cert_path: Some("/certs/ca.crt".to_string()),
            ..Default::default()
        };

        let tls = NatsConfig::from_settings(&settings, Environment::Production, None)
            .await
            .expect("resolves")
            .tls
            .expect("a tls:// URL configures TLS");

        assert_eq!(tls.ca_cert.as_deref(), Some(Path::new("/certs/ca.crt")));
    }

    /// A plain `nats://` URL with no flag stays plaintext.
    #[tokio::test]
    async fn a_plain_url_stays_plaintext() {
        let settings = NatsSettings {
            url: "nats://nats:4222".to_string(),
            ..Default::default()
        };

        assert!(
            NatsConfig::from_settings(&settings, Environment::Development, None)
                .await
                .expect("resolves")
                .tls
                .is_none()
        );
    }

    /// Half a client pair is not applied.
    #[tokio::test]
    async fn a_client_certificate_without_its_key_is_not_applied() {
        let settings = NatsSettings {
            url: "nats://nats:4222".to_string(),
            tls_enabled: true,
            tls_client_cert_path: Some("/certs/client.layer4.crt".to_string()),
            ..Default::default()
        };

        let tls = NatsConfig::from_settings(&settings, Environment::Production, None)
            .await
            .expect("resolves")
            .tls
            .expect("tls_enabled builds a TLS configuration");

        assert!(tls.client_cert.is_none());
        assert!(tls.client_key.is_none());
    }

    #[tokio::test]
    async fn from_settings_minimal() {
        let settings = NatsSettings {
            url: "nats://localhost:4222".to_string(),
            tls_enabled: false,
            auth: NatsAuth::None,
            connection_name: None,
            ..Default::default()
        };
        let cfg = NatsConfig::from_settings(&settings, Environment::Development, None)
            .await
            .expect("resolves");
        assert!(cfg.name.is_none());
        assert!(matches!(cfg.auth, NatsClientAuth::None));
        assert!(cfg.tls.is_none());
    }
}
