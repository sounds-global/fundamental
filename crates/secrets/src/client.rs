//! Authenticated HTTP client for HashiCorp Vault.

use std::future::Future;
use std::sync::{Arc, Weak};
use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use bytes::Bytes;
use fundamental_config::settings::{
    SecretRef, VaultAuth, VaultKvRef, VaultSettings, is_secret_path,
};
use fundamental_core::Error as _;
use fundamental_core::secrets::{InMemorySecret, InMemorySecretString, SecretString};
use parking_lot::RwLock;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{Value, json};
use tokio::sync::watch;
use tracing::field::Empty;
use tracing::{Span, debug, info, instrument, warn};
use zeroize::Zeroizing;

use crate::error::{SecretError, SecretResult};
use crate::kv_field::kv_field;
use crate::kv_path::segments;
use crate::lease::LeasedCredential;
use crate::pki::{IssuedCertificate, PkiMount, PkiRole};
use crate::refreshed_secret::{RefreshedSecret, Registry};
use crate::transit::{TransitKeyInfo, TransitKeyName, TransitKeyRef, TransitKeyType, TransitMount};
use crate::{KvCas, KvData, KvFields, KvPath, KvVersion, KvWritten};

/// A client's shared state; the token renewer holds it only by `Weak`.
struct Inner {
    http: Client,
    address: String,
    namespace: Option<String>,
    token: RwLock<InMemorySecretString>,
    /// Kept so a lapsed token is replaced by logging in again.
    auth: VaultAuth,
    refreshed: Registry,
    /// Logins after the first; a lease dies with the token that minted it, so its renewer moves on each change.
    logins: watch::Sender<u64>,
}

/// An authenticated client for HashiCorp Vault, cheap to clone.
#[derive(Clone)]
pub struct VaultClient {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for VaultClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultClient")
            .field("address", &self.inner.address)
            .field("namespace", &self.inner.namespace)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl VaultClient {
    /// Builds and logs in a client, or returns `Ok(None)` when the backend is disabled.
    #[instrument(skip(settings), fields(vault.address = %settings.address, enabled = settings.enabled), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn from_settings(settings: &VaultSettings) -> SecretResult<Option<Self>> {
        if !settings.enabled {
            debug!("vault backend disabled; skipping client construction");
            return Ok(None);
        }

        let mut builder = Client::builder()
            .user_agent("fundamental-secrets")
            .timeout(Duration::from_secs(30));
        if settings.tls_skip_verify {
            builder = builder.danger_accept_invalid_certs(true);
        }
        if let Some(ca_path) = &settings.ca_cert_path {
            let pem = std::fs::read(ca_path).map_err(|e| SecretError::Io {
                context: format!("reading CA cert {ca_path}"),
                source: e,
            })?;
            let cert = reqwest::Certificate::from_pem(&pem)
                .map_err(|e| SecretError::InvalidConfig(format!("invalid CA cert: {e}")))?;
            builder = builder.add_root_certificate(cert);
        }
        if let VaultAuth::Cert {
            client_cert_path,
            client_key_path,
            ..
        } = &settings.auth
        {
            builder = builder.identity(client_identity(client_cert_path, client_key_path)?);
        }
        let http = builder.build()?;

        let client = Self {
            inner: Arc::new(Inner {
                http,
                address: settings.address.trim_end_matches('/').to_string(),
                namespace: settings.namespace.clone(),
                token: RwLock::new(InMemorySecret::absent()),
                auth: settings.auth.clone(),
                refreshed: Registry::default(),
                logins: watch::Sender::new(0),
            }),
        };

        let login = client.login(&settings.auth).await?;
        *client.inner.token.write() = InMemorySecret::new(login.client_token);

        // The task holds only a Weak ref, so it ends when the last client clone is dropped.
        if login.renewable && login.lease_duration > 0 {
            client.spawn_token_renewer(login.lease_duration);
        }

        Ok(Some(client))
    }

    #[instrument(skip(self, auth), fields(method = Empty), err(Debug))]
    async fn login(&self, auth: &VaultAuth) -> SecretResult<LoginOutcome> {
        let method = match auth {
            VaultAuth::Token { .. } => "token",
            VaultAuth::AppRole { .. } => "approle",
            VaultAuth::Kubernetes { .. } => "kubernetes",
            VaultAuth::Cert { .. } => "cert",
        };
        Span::current().record("method", method);
        match auth {
            VaultAuth::Token { token } => Ok(LoginOutcome {
                client_token: token
                    .expose()
                    .ok_or_else(|| {
                        SecretError::Auth(
                            "token auth selected but no Vault token is configured".to_string(),
                        )
                    })?
                    .to_string(),
                lease_duration: 0,
                renewable: false,
            }),
            VaultAuth::AppRole { role_id, secret_id } => {
                let secret_id = secret_id.expose().ok_or_else(|| {
                    SecretError::Auth(
                        "approle auth selected but no secret_id is configured".to_string(),
                    )
                })?;
                let body = json!({ "role_id": role_id, "secret_id": secret_id });
                let resp = self.send_unauth("auth/approle/login", &body).await?;
                LoginOutcome::from_auth(&resp, "auth/approle/login")
            }
            VaultAuth::Kubernetes { role, jwt_path } => {
                let jwt = std::fs::read_to_string(jwt_path).map_err(|e| SecretError::Io {
                    context: format!("reading k8s jwt {jwt_path}"),
                    source: e,
                })?;
                let body = json!({ "role": role, "jwt": jwt.trim() });
                let resp = self.send_unauth("auth/kubernetes/login", &body).await?;
                LoginOutcome::from_auth(&resp, "auth/kubernetes/login")
            }
            // The credential is the certificate presented in the handshake; the body names its role.
            VaultAuth::Cert { role, .. } => {
                let body = role
                    .as_ref()
                    .map_or_else(|| json!({}), |role| json!({ "name": role }));
                let resp = self.send_unauth("auth/cert/login", &body).await?;
                LoginOutcome::from_auth(&resp, "auth/cert/login")
            }
        }
    }

    /// The number of logins after the first; leases are rotated when it changes.
    pub(crate) fn logins(&self) -> watch::Receiver<u64> {
        self.inner.logins.subscribe()
    }

    /// Keeps the client token alive while a client exists.
    ///
    /// A grant short of the ask means the token is near its maximum TTL, so the client logs in
    /// again while a third of the TTL remains, leaving lease renewers time to move onto the new token.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn spawn_token_renewer(&self, lease_duration: u64) {
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            let mut requested = lease_duration.max(1);
            let mut ttl = requested;
            loop {
                tokio::time::sleep(renew_delay(ttl)).await;
                let Some(inner) = weak.upgrade() else { break };
                let client = VaultClient { inner };
                let granted = match client
                    .put("auth/token/renew-self", &json!({ "increment": requested }))
                    .await
                {
                    Ok(resp) => {
                        let granted = resp.pointer("/auth/lease_duration").and_then(Value::as_u64);
                        if granted.is_none() {
                            warn!(
                                "vault token renewal answered without a lease duration; logging in again"
                            );
                        }
                        granted
                    }
                    Err(error) => {
                        warn!(%error, "vault token renewal failed; logging in again");
                        None
                    }
                };
                match granted {
                    Some(granted) if granted >= requested => {
                        ttl = granted;
                        debug!(ttl, "renewed vault client token");
                    }
                    _ => {
                        if let Some(granted) = granted {
                            info!(
                                granted,
                                requested,
                                "vault client token is near its maximum TTL; logging in before it expires"
                            );
                        }
                        match client.login_until_success().await {
                            Some(lease) => {
                                requested = lease.max(1);
                                ttl = requested;
                            }
                            None => break,
                        }
                    }
                }
                drop(client);
            }
        });
    }

    /// Logs in again until it succeeds, returning the new lease duration, or `None` for a
    /// non-renewable token, which ends the renewer.
    #[expect(
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    async fn login_until_success(&self) -> Option<u64> {
        let mut backoff = Duration::from_secs(1);
        loop {
            match self.login(&self.inner.auth).await {
                Ok(login) => {
                    *self.inner.token.write() = InMemorySecret::new(login.client_token);
                    self.inner.logins.send_modify(|count| *count += 1);
                    if login.renewable && login.lease_duration > 0 {
                        info!(
                            lease_duration = login.lease_duration,
                            "vault client logged in again"
                        );
                        return Some(login.lease_duration);
                    }
                    warn!("vault login returned a non-renewable token; the renewer stops");
                    return None;
                }
                Err(error) => {
                    warn!(%error, backoff_secs = backoff.as_secs(), "vault login failed; retrying");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(60));
                }
            }
        }
    }

    /// Creates a non-exportable signing key if it does not exist, and returns the key as held.
    ///
    /// An existing key is returned whatever type was asked for, so the caller checks
    /// [`TransitKeyInfo::key_type`].
    ///
    /// # Errors
    /// [`SecretError::Api`] if the engine refuses, or [`SecretError::UnexpectedResponse`]
    /// if its answer lacks a field this client relies on.
    #[instrument(skip(self, name), fields(vault.mount = %mount, key_type = %key_type), err(Debug))]
    pub async fn transit_ensure_signing_key(
        &self,
        mount: &TransitMount,
        name: &TransitKeyName,
        key_type: TransitKeyType,
    ) -> SecretResult<TransitKeyInfo> {
        let path = format!("{}/keys/{}", mount.as_str(), name.as_str());
        let body = json!({
            "type": key_type.as_str(),
            "exportable": false,
            "allow_plaintext_backup": false,
        });
        let created = self.post(&path, &body).await?;
        if created.get("data").is_some() {
            return TransitKeyInfo::from_response(&created, &path);
        }
        // An engine that answers `204 No Content` to a create is read back.
        self.transit_read_signing_key(mount, name).await
    }

    /// Reads a signing key's type, export settings and public keys.
    ///
    /// # Errors
    /// [`SecretError::Api`] with status 404 if the key does not exist, or another
    /// [`SecretError`] on transport failure or an unusable answer.
    #[instrument(skip(self, name), fields(vault.mount = %mount), err(Debug))]
    pub async fn transit_read_signing_key(
        &self,
        mount: &TransitMount,
        name: &TransitKeyName,
    ) -> SecretResult<TransitKeyInfo> {
        let path = format!("{}/keys/{}", mount.as_str(), name.as_str());
        let resp = self.get(&path).await?;
        TransitKeyInfo::from_response(&resp, &path)
    }

    /// Signs `message` with the pinned key version, returning the engine's
    /// `vault:vN:<signature>` string unparsed.
    ///
    /// # Errors
    /// [`SecretError::Api`] if the engine refuses, or another [`SecretError`] on transport
    /// failure or an unusable answer.
    #[instrument(skip(self, key, extra, message), fields(vault.mount = %key.mount, key_version = key.version.get()), err(Debug))]
    pub async fn transit_sign(
        &self,
        key: &TransitKeyRef,
        path_suffix: &str,
        mut extra: serde_json::Map<String, Value>,
        message: &[u8],
    ) -> SecretResult<String> {
        let path = format!(
            "{}/sign/{}{}",
            key.mount.as_str(),
            key.name.as_str(),
            path_suffix
        );
        extra.insert("input".to_owned(), Value::from(STANDARD.encode(message)));
        extra.insert("key_version".to_owned(), Value::from(key.version.get()));
        let resp = self.post(&path, &Value::Object(extra)).await?;
        resp.pointer("/data/signature")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| SecretError::UnexpectedResponse {
                path,
                message: "missing data.signature".to_owned(),
            })
    }

    /// Decrypts a Vault Transit ciphertext (`vault:v1:…`), returning raw plaintext.
    #[instrument(skip(self, ciphertext), fields(mount, key), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn transit_decrypt(
        &self,
        mount: &str,
        key: &str,
        ciphertext: &str,
    ) -> SecretResult<Vec<u8>> {
        let path = format!("{}/decrypt/{}", mount.trim_matches('/'), key);
        let resp = self
            .post(&path, &json!({ "ciphertext": ciphertext }))
            .await?;
        let b64 = resp
            .pointer("/data/plaintext")
            .and_then(Value::as_str)
            .ok_or_else(|| SecretError::UnexpectedResponse {
                path: path.clone(),
                message: "missing data.plaintext".to_string(),
            })?;
        STANDARD
            .decode(b64)
            .map_err(|e| SecretError::UnexpectedResponse {
                path,
                message: format!("invalid base64 plaintext: {e}"),
            })
    }

    /// Encrypts raw plaintext with a Vault Transit key, returning the `vault:v1:…` ciphertext.
    #[instrument(skip(self, plaintext), fields(mount, key), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn transit_encrypt(
        &self,
        mount: &str,
        key: &str,
        plaintext: &[u8],
    ) -> SecretResult<String> {
        let path = format!("{}/encrypt/{}", mount.trim_matches('/'), key);
        let body = json!({ "plaintext": STANDARD.encode(plaintext) });
        let resp = self.post(&path, &body).await?;
        resp.pointer("/data/ciphertext")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| SecretError::UnexpectedResponse {
                path,
                message: "missing data.ciphertext".to_string(),
            })
    }

    /// Signs `csr_pem` through the PKI role `role` mounted at `mount`, for `ttl`.
    ///
    /// The role bounds what the certificate may say, and may shorten `ttl`.
    ///
    /// # Errors
    /// [`SecretError::InvalidConfig`] when `csr_pem` is not a PEM certificate request of at
    /// most 16 KiB; the request errors when custody refuses or cannot be reached; and
    /// [`SecretError::UnexpectedResponse`] when its answer is not a whole certificate.
    #[instrument(skip(self, csr_pem), fields(pki.mount = %mount, pki.role = %role, ttl_secs = ttl.as_secs(), serial = Empty), err(Debug))]
    pub async fn pki_sign(
        &self,
        mount: &PkiMount,
        role: &PkiRole,
        csr_pem: &str,
        ttl: Duration,
    ) -> SecretResult<IssuedCertificate> {
        const MAX_CSR_BYTES: usize = 16 * 1024;
        let trimmed = csr_pem.trim();
        if trimmed.len() > MAX_CSR_BYTES
            || !trimmed.starts_with("-----BEGIN CERTIFICATE REQUEST-----")
            || !trimmed.ends_with("-----END CERTIFICATE REQUEST-----")
        {
            return Err(SecretError::InvalidConfig(format!(
                "a certificate request is one PEM `CERTIFICATE REQUEST` of at most {MAX_CSR_BYTES} bytes"
            )));
        }
        let path = format!("{mount}/sign/{role}");
        let body = json!({ "csr": trimmed, "ttl": format!("{}s", ttl.as_secs()), "format": "pem" });
        let response = self.post(&path, &body).await?;
        let issued = IssuedCertificate::from_response(&response, &path)?;
        Span::current().record("serial", issued.serial_number.as_str());
        info!("custody signed a certificate through its role");
        Ok(issued)
    }

    /// Reads a KV v2 secret, returning its `data.data` map.
    #[instrument(skip(self), fields(mount, path), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn read_kv(
        &self,
        mount: &str,
        path: &str,
    ) -> SecretResult<serde_json::Map<String, Value>> {
        let full = format!(
            "{}/data/{}",
            mount.trim_matches('/'),
            path.trim_matches('/')
        );
        let resp = self.get(&full).await?;
        resp.pointer("/data/data")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| SecretError::UnexpectedResponse {
                path: full,
                message: "missing data.data".to_string(),
            })
    }

    /// The secret `reference` names: the value itself, or the one KV v2 field it points at.
    ///
    /// Only that field is decoded, from a bounded buffer wiped on drop.
    /// # Errors
    /// - [`SecretError::InvalidConfig`] when a mount, path or key would not name one secret —
    ///   an empty or `..` segment, or a character outside the path alphabet.
    /// - [`SecretError::MissingField`] when the secret holds no string field `key`.
    /// - [`SecretError::Api`] when the store refuses, [`SecretError::Transport`] when it cannot
    ///   be reached, [`SecretError::UnexpectedResponse`] when its answer is too large or not a
    ///   key-value read.
    #[instrument(skip_all, fields(vault.mount, vault.path, vault.key), err(Debug))]
    pub async fn read_secret(&self, reference: &SecretRef) -> SecretResult<SecretString> {
        match reference {
            SecretRef::Value(value) => Ok(value.clone()),
            SecretRef::VaultKv(kv) => self.read_kv_field(kv).await,
        }
    }

    /// The secret `reference` names, re-read every `interval` and shared by every caller of
    /// this client; the first caller's `interval` sets the pace.
    ///
    /// # Errors
    /// [`SecretError::InvalidConfig`] for an `interval` under [`MIN_REFRESH_INTERVAL`], which
    /// would turn the refresher into a loop against the store; otherwise what
    /// [`read_secret`](Self::read_secret) returns for the first read.
    pub async fn refreshed_secret(
        &self,
        reference: &VaultKvRef,
        interval: Duration,
    ) -> SecretResult<RefreshedSecret> {
        if interval < MIN_REFRESH_INTERVAL {
            return Err(SecretError::InvalidConfig(format!(
                "a secret is refreshed at most once a second; {interval:?} was asked for {reference}"
            )));
        }
        if let Some(shared) = RefreshedSecret::registered(&self.inner.refreshed, reference) {
            debug!(secret.source = %reference, "secret already refreshed for this client; shared");
            return Ok(shared);
        }
        let started = RefreshedSecret::start(self.clone(), reference.clone(), interval).await?;
        Ok(started.register(&self.inner.refreshed))
    }

    async fn read_kv_field(&self, kv: &VaultKvRef) -> SecretResult<SecretString> {
        let span = Span::current();
        span.record("vault.mount", kv.mount.as_str());
        span.record("vault.path", kv.path.as_str());
        span.record("vault.key", kv.key.as_str());
        let path = format!(
            "{}/data/{}",
            segments(&kv.mount, "mount")?,
            segments(&kv.path, "path")?
        );
        if kv.key.is_empty() || kv.key.chars().any(char::is_control) {
            return Err(SecretError::InvalidConfig(
                "a secret's key is not empty and holds no control character".to_owned(),
            ));
        }
        let body = self.get_bounded(&path, None).await?;
        match kv_field(&body, &kv.key) {
            Ok(Some(value)) => {
                debug!(vault.path = %path, "secret read");
                Ok(SecretString::new(value))
            }
            Ok(None) => Err(SecretError::MissingField {
                path,
                key: kv.key.clone(),
            }),
            Err(error) => Err(unreadable(path, &kv.key, &error)),
        }
    }

    /// Writes `data` as a new version of the KV v2 secret at `path`, if its current version
    /// satisfies `cas`.
    ///
    /// Sent once and never retried: a write that lost its answer would land twice.
    /// # Errors
    /// - [`SecretError::WriteTooLarge`] before anything is sent, when the body would exceed
    ///   [`KV_BODY_LIMIT`].
    /// - [`SecretError::CasMismatch`] when the current version does not satisfy `cas`.
    /// - [`SecretError::Api`] when the store refuses, [`SecretError::Transport`] when it cannot
    ///   be reached, [`SecretError::UnexpectedResponse`] when it answers without a version.
    #[instrument(skip_all, fields(vault.path = %path, kv.cas = ?cas, kv.keys = ?data.keys().collect::<Vec<_>>(), http.status = Empty, kv.version = Empty), err(Debug))]
    pub async fn write_kv(
        &self,
        path: &KvPath,
        data: &KvData<'_>,
        cas: KvCas,
    ) -> SecretResult<KvWritten> {
        let endpoint = path.data();
        let mut body = Zeroizing::new(Vec::with_capacity(KV_BODY_LIMIT));
        if data.encode(cas, &mut body).is_err() {
            warn!(vault.path = %endpoint, limit = KV_BODY_LIMIT, "a key-value write exceeds the limit; refused unsent");
            return Err(SecretError::WriteTooLarge {
                path: endpoint,
                limit: KV_BODY_LIMIT,
            });
        }
        let req = self.with_headers(
            self.inner
                .http
                .post(self.endpoint(&endpoint))
                .header(CONTENT_TYPE, "application/json")
                .body(Bytes::from_owner(body)),
            true,
        );
        let resp = req.send().await?;
        let status = resp.status();
        Span::current().record("http.status", status.as_u16());
        let text = resp.text().await?;
        if !status.is_success() {
            let message = parse_vault_errors(&text);
            if status == StatusCode::BAD_REQUEST && message.contains("check-and-set") {
                info!(vault.path = %endpoint, "a key-value write lost its check-and-set");
                return Err(SecretError::CasMismatch { path: endpoint });
            }
            warn!(http.status = status.as_u16(), vault.path = %endpoint, "vault refused a key-value write");
            return Err(SecretError::api(status.as_u16(), endpoint, message));
        }
        let version = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|answer| answer.pointer("/data/version").and_then(Value::as_u64))
            .and_then(KvVersion::new)
            .ok_or_else(|| SecretError::UnexpectedResponse {
                path: endpoint.clone(),
                message: "a write answered without data.version".to_owned(),
            })?;
        Span::current().record("kv.version", version.get());
        info!(vault.path = %endpoint, kv.version = version.get(), "key-value secret written");
        Ok(KvWritten { version })
    }

    /// The fields named `keys` of `version` of the KV secret at `path`, and nothing else.
    ///
    /// # Errors
    /// - [`SecretError::VersionGone`] when the version was never written, or was deleted or
    ///   destroyed.
    /// - [`SecretError::Api`] when the store refuses, [`SecretError::Transport`] when it cannot
    ///   be reached, [`SecretError::UnexpectedResponse`] when its answer is too large or not a
    ///   key-value read.
    #[instrument(skip_all, fields(vault.path = %path, kv.version = version.get(), kv.keys = ?keys), err(Debug))]
    pub async fn read_kv_fields(
        &self,
        path: &KvPath,
        version: KvVersion,
        keys: &[&str],
    ) -> SecretResult<KvFields> {
        let endpoint = path.data();
        let gone = |endpoint: String| SecretError::VersionGone {
            path: endpoint,
            version: version.get(),
        };
        let body = match self
            .get_bounded(&endpoint, Some(("version", version.get())))
            .await
        {
            Err(SecretError::Api { status: 404, .. }) => {
                info!(vault.path = %endpoint, kv.version = version.get(), "the version asked for is gone");
                return Err(gone(endpoint));
            }
            other => other?,
        };
        match KvFields::parse(&body, keys) {
            Ok(Some(fields)) => {
                debug!(vault.path = %endpoint, found = fields.keys().count(), "key-value fields read");
                Ok(fields)
            }
            Ok(None) => {
                info!(vault.path = %endpoint, kv.version = version.get(), "the version asked for holds no data");
                Err(gone(endpoint))
            }
            Err(error) => Err(unreadable(endpoint, "data", &error)),
        }
    }

    /// Destroys `version` of the KV secret at `path`; an absent version succeeds.
    ///
    /// # Errors
    /// [`SecretError::Api`] when the store refuses, [`SecretError::Transport`] when it cannot be
    /// reached.
    #[instrument(skip_all, fields(vault.path = %path, kv.version = version.get()), err(Debug))]
    pub async fn destroy_kv_version(&self, path: &KvPath, version: KvVersion) -> SecretResult<()> {
        self.put(&path.destroy(), &json!({ "versions": [version.get()] }))
            .await?;
        info!(vault.path = %path, kv.version = version.get(), "key-value version destroyed");
        Ok(())
    }

    /// The current version of the KV secret at `path`, or `None` when none was ever written.
    ///
    /// # Errors
    /// [`SecretError::Api`] when the store refuses, [`SecretError::Transport`] when it cannot be
    /// reached, [`SecretError::UnexpectedResponse`] when it answers without a current version.
    #[instrument(skip_all, fields(vault.path = %path, kv.version = Empty), err(Debug))]
    pub async fn read_kv_current_version(&self, path: &KvPath) -> SecretResult<Option<KvVersion>> {
        let endpoint = path.metadata();
        let answer = match self.get(&endpoint).await {
            Err(SecretError::Api { status: 404, .. }) => {
                debug!(vault.path = %endpoint, "the secret has no metadata");
                return Ok(None);
            }
            other => other?,
        };
        let current = answer
            .pointer("/data/current_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| SecretError::UnexpectedResponse {
                path: endpoint.clone(),
                message: "metadata answered without data.current_version".to_owned(),
            })?;
        Span::current().record("kv.version", current);
        Ok(KvVersion::new(current))
    }

    /// Destroys every version of the KV secret at `path` and its metadata; an absent secret succeeds.
    ///
    /// # Errors
    /// [`SecretError::Api`] when the store refuses, [`SecretError::Transport`] when it cannot be
    /// reached.
    #[instrument(skip_all, fields(vault.path = %path), err(Debug))]
    pub async fn destroy_kv(&self, path: &KvPath) -> SecretResult<()> {
        let endpoint = path.metadata();
        let req = self.with_headers(self.inner.http.delete(self.endpoint(&endpoint)), true);
        self.execute(req, &endpoint).await?;
        info!(vault.path = %path, "key-value secret destroyed with every version");
        Ok(())
    }

    /// Mints a short-lived database credential from `mount`/`role`.
    #[instrument(skip(self), fields(mount, role), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn dynamic_db_creds(
        &self,
        mount: &str,
        role: &str,
    ) -> SecretResult<LeasedCredential> {
        let path = format!("{}/creds/{}", mount.trim_matches('/'), role);
        // Read before the request: a login during it only makes the credential move once more.
        let minted_under = *self.inner.logins.borrow();
        let resp = self.get_retrying_transient(&path).await?;
        LeasedCredential::from_response(&path, &resp, minted_under)
    }

    /// Renews a lease by id, returning the lease duration Vault granted (seconds).
    ///
    /// # Errors
    /// [`SecretError::InvalidConfig`] for an id that is not a lease path;
    /// [`SecretError::Api`] when Vault refuses; [`SecretError::UnexpectedResponse`] when it
    /// answers without the granted duration.
    #[instrument(skip(self), fields(lease_id, increment), err(Debug))]
    pub async fn renew_lease(&self, lease_id: &str, increment: u64) -> SecretResult<u64> {
        let path = format!("sys/leases/renew/{}", lease_path(lease_id)?);
        let resp = self.put(&path, &json!({ "increment": increment })).await?;
        resp.get("lease_duration")
            .and_then(Value::as_u64)
            .ok_or_else(|| SecretError::UnexpectedResponse {
                path,
                message: "missing lease_duration".to_string(),
            })
    }

    /// Revokes a lease by id, ending the credential it leased.
    ///
    /// The id travels in the path so a policy can scope revocation to one mount and role.
    ///
    /// # Errors
    /// [`SecretError::InvalidConfig`] for an id that is not a lease path;
    /// [`SecretError::Api`] when Vault refuses.
    ///
    /// Retried like a mint, including Vault's `400` for a revocation its database refused.
    #[instrument(skip(self), fields(lease_id), err(Debug))]
    pub async fn revoke_lease(&self, lease_id: &str) -> SecretResult<()> {
        let path = format!("sys/leases/revoke/{}", lease_path(lease_id)?);
        let empty = json!({});
        retrying(
            &path,
            |failure| is_transient(failure) || is_refused_revocation(failure),
            || self.put(&path, &empty),
        )
        .await
        .map(drop)
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/v1/{}", self.inner.address, path.trim_start_matches('/'))
    }

    fn with_headers(&self, mut req: RequestBuilder, with_token: bool) -> RequestBuilder {
        if with_token {
            let token = self.inner.token.read();
            if let Some(token) = token.expose().filter(|token| !token.is_empty()) {
                req = req.header("X-Vault-Token", token.as_str());
            }
        }
        if let Some(ns) = &self.inner.namespace {
            req = req.header("X-Vault-Namespace", ns.clone());
        }
        req
    }

    /// The single HTTP boundary every Vault operation flows through; headers are never logged.
    #[instrument(skip(self, req), fields(vault.path = %path, http.status = Empty), err(Debug))]
    async fn execute(&self, req: RequestBuilder, path: &str) -> SecretResult<Value> {
        let resp = req.send().await?;
        let status = resp.status();
        Span::current().record("http.status", status.as_u16());
        let text = resp.text().await?;
        if !status.is_success() {
            warn!(
                http.status = status.as_u16(),
                vault.path = %path,
                "vault returned a non-success status"
            );
            return Err(SecretError::api(
                status.as_u16(),
                path,
                parse_vault_errors(&text),
            ));
        }
        debug!(http.status = status.as_u16(), "vault request succeeded");
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| SecretError::UnexpectedResponse {
            path: path.to_string(),
            message: e.to_string(),
        })
    }

    /// A `GET` retried on a bounded backoff while the failure is transient.
    ///
    /// Concurrent replicas creating a role make the database refuse all but one catalog write.
    async fn get_retrying_transient(&self, path: &str) -> SecretResult<Value> {
        retrying(path, is_transient, || self.get(path)).await
    }

    async fn get(&self, path: &str) -> SecretResult<Value> {
        let req = self.with_headers(self.inner.http.get(self.endpoint(path)), true);
        self.execute(req, path).await
    }

    /// A `GET` read into a fixed buffer of [`KV_BODY_LIMIT`] bytes, wiped on drop.
    async fn get_bounded(
        &self,
        path: &str,
        query: Option<(&str, u64)>,
    ) -> SecretResult<Zeroizing<Vec<u8>>> {
        let mut req = self.inner.http.get(self.endpoint(path));
        if let Some(query) = query {
            req = req.query(&[query]);
        }
        let resp = self.with_headers(req, true).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            warn!(http.status = status.as_u16(), vault.path = %path, "vault refused a secret read");
            return Err(SecretError::api(
                status.as_u16(),
                path,
                parse_vault_errors(&text),
            ));
        }
        drain_bounded(resp, path).await
    }

    async fn post(&self, path: &str, body: &Value) -> SecretResult<Value> {
        let req = self.with_headers(self.inner.http.post(self.endpoint(path)).json(body), true);
        self.execute(req, path).await
    }

    async fn put(&self, path: &str, body: &Value) -> SecretResult<Value> {
        let req = self.with_headers(self.inner.http.put(self.endpoint(path)).json(body), true);
        self.execute(req, path).await
    }

    async fn send_unauth(&self, path: &str, body: &Value) -> SecretResult<Value> {
        let req = self.with_headers(self.inner.http.post(self.endpoint(path)).json(body), false);
        self.execute(req, path).await
    }
}

/// The TLS client identity read from the configured certificate and key files.
fn client_identity(cert_path: &str, key_path: &str) -> SecretResult<reqwest::Identity> {
    let read = |path: &str| {
        std::fs::read(path).map_err(|e| SecretError::Io {
            context: format!("reading the vault client identity {path}"),
            source: e,
        })
    };
    let mut pem = read(cert_path)?;
    pem.push(b'\n');
    pem.extend(read(key_path)?);
    reqwest::Identity::from_pem(&pem).map_err(|e| {
        SecretError::InvalidConfig(format!(
            "the vault client identity is not a PEM certificate and key: {e}"
        ))
    })
}

struct LoginOutcome {
    client_token: String,
    lease_duration: u64,
    renewable: bool,
}

impl LoginOutcome {
    fn from_auth(resp: &Value, path: &str) -> SecretResult<Self> {
        let auth = resp
            .get("auth")
            .ok_or_else(|| SecretError::Auth(format!("no auth block in response from {path}")))?;
        let client_token = auth
            .get("client_token")
            .and_then(Value::as_str)
            .ok_or_else(|| SecretError::Auth(format!("no client_token from {path}")))?
            .to_string();
        Ok(Self {
            client_token,
            lease_duration: auth
                .get("lease_duration")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            renewable: auth
                .get("renewable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }
}

/// `lease_id` as a path suffix, refusing anything that could name a different endpoint.
fn lease_path(lease_id: &str) -> SecretResult<&str> {
    if is_secret_path(lease_id) {
        Ok(lease_id)
    } else {
        Err(SecretError::InvalidConfig(format!(
            "{lease_id:?} is not a lease id"
        )))
    }
}

/// The shortest interval a secret is refreshed at: one read a second per secret per client.
pub const MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// The largest key-value answer a secret read accepts.
pub const KV_BODY_LIMIT: usize = 64 * 1024;

/// `resp`'s body in a fixed buffer of [`KV_BODY_LIMIT`] bytes, wiped on drop.
async fn drain_bounded(
    mut resp: reqwest::Response,
    path: &str,
) -> SecretResult<Zeroizing<Vec<u8>>> {
    let mut body = Zeroizing::new(Vec::with_capacity(KV_BODY_LIMIT));
    while let Some(chunk) = resp.chunk().await? {
        if body.len() + chunk.len() > KV_BODY_LIMIT {
            warn!(vault.path = %path, limit = KV_BODY_LIMIT, "a secret read answered more than the limit; refused unread");
            return Err(SecretError::UnexpectedResponse {
                path: path.to_owned(),
                message: format!("the answer exceeds {KV_BODY_LIMIT} bytes"),
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// An unreadable KV answer, described without the parser's message, which quotes the secret.
fn unreadable(path: String, key: &str, error: &serde_json::Error) -> SecretError {
    SecretError::UnexpectedResponse {
        path,
        message: format!(
            "not a key-value read with a string `{key}`: a {:?} error at line {}, column {}",
            error.classify(),
            error.line(),
            error.column()
        ),
    }
}

/// How many times a mint is attempted before its failure is returned.
const MINT_ATTEMPTS: u32 = 5;

/// The first wait between mint attempts, doubled after each.
const MINT_BACKOFF: Duration = Duration::from_millis(200);

fn is_transient(failure: &SecretError) -> bool {
    failure.is_retryable()
}

/// Whether `failure` is Vault reporting that the backend refused a revocation — its `400`
/// with `failed to revoke entry` — rather than refusing the request itself.
fn is_refused_revocation(failure: &SecretError) -> bool {
    matches!(failure, SecretError::Api { status: 400, message, .. } if message.contains("failed to revoke"))
}

/// Runs `request` up to [`MINT_ATTEMPTS`] times with doubling backoff while `retryable` accepts its failure.
async fn retrying<F, Fut>(
    path: &str,
    retryable: impl Fn(&SecretError) -> bool,
    mut request: F,
) -> SecretResult<Value>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = SecretResult<Value>>,
{
    let mut backoff = MINT_BACKOFF;
    for attempt in 1..MINT_ATTEMPTS {
        match request().await {
            Err(failure) if retryable(&failure) => {
                warn!(
                    vault.path = %path,
                    attempt,
                    code = fundamental_core::Error::error_code(&failure),
                    error = %failure,
                    backoff_ms = backoff.as_millis(),
                    "vault request failed transiently; retrying"
                );
                tokio::time::sleep(backoff).await;
                backoff *= 2;
            }
            outcome => return outcome,
        }
    }
    request().await
}

/// Waits until ~2/3 of the TTL before renewing.
fn renew_delay(ttl: u64) -> Duration {
    Duration::from_secs(((ttl.max(1) * 2) / 3).max(1))
}

fn parse_vault_errors(text: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(text)
        && let Some(errs) = v.get("errors").and_then(Value::as_array)
    {
        let joined = errs
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("; ");
        if !joined.is_empty() {
            return joined;
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "(empty body)".to_string()
    } else {
        trimmed.chars().take(200).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_config::settings::VaultSettings;

    #[test]
    fn a_secret_path_names_one_secret_or_is_refused() {
        assert_eq!(segments("/nats/mijn/", "path").ok(), Some("nats/mijn"));
        assert_eq!(segments("a7d", "mount").ok(), Some("a7d"));
        for hostile in [
            "",
            "/",
            "nats/../other",
            "nats//mijn",
            "nats/./mijn",
            "nats/mi jn",
            "nats?list=true",
        ] {
            assert!(
                matches!(
                    segments(hostile, "path"),
                    Err(SecretError::InvalidConfig(_))
                ),
                "{hostile:?} must be refused"
            );
        }
    }
    #[tokio::test]
    async fn from_settings_returns_none_when_disabled() {
        let settings = VaultSettings::default(); // enabled = false
        let client = VaultClient::from_settings(&settings).await.unwrap();
        assert!(client.is_none());
    }

    #[test]
    fn parse_vault_errors_extracts_messages() {
        let body = r#"{"errors":["permission denied","missing role"]}"#;
        assert_eq!(parse_vault_errors(body), "permission denied; missing role");
    }

    #[test]
    fn parse_vault_errors_falls_back_to_raw() {
        assert_eq!(parse_vault_errors(""), "(empty body)");
        assert_eq!(parse_vault_errors("upstream timeout"), "upstream timeout");
    }

    #[test]
    fn a_lease_id_reaches_the_path_only_when_it_is_one() {
        assert_eq!(
            lease_path("database/creds/app/AbC.12-x_y").ok(),
            Some("database/creds/app/AbC.12-x_y")
        );
        for hostile in [
            "",
            "database/creds/../../sys/seal",
            "database//creds/app/x",
            "database/creds/app/x?y=1",
            "database/creds/app/x#y",
            "./database/creds/app/x",
            "database/creds/app/x/",
        ] {
            assert!(
                matches!(lease_path(hostile), Err(SecretError::InvalidConfig(_))),
                "{hostile:?} must not reach a URL path"
            );
        }
    }

    #[test]
    fn only_a_request_that_may_succeed_on_a_repeat_is_retried() {
        let api = |status| SecretError::api(status, "database/creds/app", "");
        assert!(is_transient(&api(500)));
        assert!(is_transient(&api(503)));
        assert!(is_transient(&api(429)));
        assert!(!is_transient(&api(403)));
        assert!(!is_transient(&api(400)));
        assert!(!is_transient(&SecretError::InvalidConfig(String::new())));
    }

    #[test]
    fn a_revocation_the_backend_refused_is_retried_and_a_forbidden_one_is_not() {
        let api = |status, message: &str| {
            SecretError::api(status, "sys/leases/revoke/database/creds/app/x", message)
        };
        assert!(is_refused_revocation(&api(
            400,
            "failed to revoke entry: resp: nil err: 40001"
        )));
        assert!(!is_refused_revocation(&api(400, "invalid lease")));
        assert!(!is_refused_revocation(&api(403, "permission denied")));
    }

    #[test]
    fn renew_delay_is_two_thirds_of_ttl() {
        assert_eq!(renew_delay(30), Duration::from_secs(20));
        assert_eq!(renew_delay(1), Duration::from_secs(1));
        assert_eq!(renew_delay(0), Duration::from_secs(1));
    }

    #[test]
    fn io_error_preserves_source_chain_and_context() {
        use std::error::Error as _;
        use std::io::{Error as IoError, ErrorKind};

        let err = SecretError::Io {
            context: "reading CA cert /etc/vault/ca.pem".to_string(),
            source: IoError::new(ErrorKind::NotFound, "no such file"),
        };
        assert!(format!("{err}").contains("reading CA cert"));
        let source = err.source().expect("Io must expose its source");
        assert!(source.to_string().contains("no such file"));
    }
}
