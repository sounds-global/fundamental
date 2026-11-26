//! Session wrapper around the underlying YCQL driver session.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;
use rustls::pki_types::pem::{Error as PemError, PemObject};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use scylla::client::Compression;
use scylla::client::execution_profile::ExecutionProfile;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::policies::load_balancing::DefaultPolicy;
use tracing::debug;

use fundamental_db::TlsConfig;

use super::address_translation::{AddressTranslation, TranslationTarget};
use super::pinned_server_name::PinnedServerName;
use crate::{ClusterInfo, CqlConfig, CqlError, CqlResult, NodeInfo};

/// A wrapper around the YCQL session.
#[derive(Debug, Clone)]
pub struct CqlSession {
    /// The underlying session, shared by every clone and replaceable under all of them.
    inner: Arc<ArcSwap<Session>>,
    config: Arc<CqlConfig>,
}

impl CqlSession {
    /// Creates a new session from configuration.
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub async fn connect(config: &CqlConfig) -> CqlResult<Self> {
        let mut builder = SessionBuilder::new()
            .known_nodes(&config.contact_points)
            .connection_timeout(config.connect_timeout)
            // Off: the agreement wait reads `system_schema.scylla_keyspaces`, which YCQL lacks, and
            // YCQL's DDL is synchronous through the master anyway.
            .auto_await_schema_agreement(false)
            // The schema metadata is the routing map, and fetching it fails on YCQL (no
            // `system_schema.scylla_tables`), so it follows the routing flag.
            .fetch_schema_metadata(config.token_aware_routing)
            // The full fetch too, or the driver warns at every boot that the two disagree.
            .fetch_full_schema_metadata(config.token_aware_routing);

        if let Some(ref keyspace) = config.default_keyspace {
            builder = builder.use_keyspace(keyspace, false);
        }

        // One profile: a second `default_execution_profile_handle` replaces the first rather than
        // merging. A policy is always built, so `token_aware_routing` applies with or without a
        // named datacenter.
        let mut profile = ExecutionProfile::builder().request_timeout(Some(config.request_timeout));
        let mut policy = DefaultPolicy::builder().token_aware(config.token_aware_routing);
        if let Some(ref datacenter) = config.local_datacenter {
            debug!(%datacenter, "preferring a local datacenter");
            policy = policy.prefer_datacenter(datacenter.clone());
        }
        debug!(
            token_aware = config.token_aware_routing,
            "applying the load-balancing policy"
        );
        profile = profile.load_balancing_policy(policy.build());
        debug!(timeout = ?config.request_timeout, "applying the request timeout");
        builder = builder.default_execution_profile_handle(profile.build().into_handle());

        if config.compression {
            builder = builder.compression(Some(Compression::Lz4));
        }

        // Only the fact that auth is in use is logged: the username can be a Vault-leased secret.
        if let (Some(username), Some(password)) = (&config.username, &config.password) {
            debug!("using authentication credentials");
            builder = builder.user(username.expose(), password.expose());
        }

        // The driver dials each node by the address it advertises, never the contact point, so
        // behind NAT or a port-forward those need rewriting.
        let translation =
            AddressTranslation::new(address_translation(&config.address_translation)?);
        if !translation.is_empty() {
            debug!(rules = translation.len(), "translating node addresses");
            builder = builder.address_translator(Arc::new(translation));
        }

        // Without this the driver connects in plaintext whatever `config.tls` says.
        if config.tls.mode.is_enabled() {
            debug!(mode = ?config.tls.mode, "Enabling TLS for YCQL connection");
            let tls = build_tls_config(&config.tls)?;
            builder = builder.tls_context(Some(std::sync::Arc::new(tls)));
        }

        debug!(
            contact_points = ?config.contact_points,
            has_credentials = config.has_credentials(),
            "Connecting to YCQL"
        );

        let session = builder
            .build()
            .await
            .map_err(|e| CqlError::Session(Box::new(e)))?;

        debug!("Connected to YCQL");

        Ok(Self {
            inner: Arc::new(ArcSwap::from_pointee(session)),
            config: Arc::new(config.clone()),
        })
    }

    /// Creates a new session with startup retry.
    ///
    /// Uses the `startup_retry` config if present, otherwise falls back to
    /// a default retry configuration for initial connection attempts.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fundamental_cql::{RetryConfig, CqlConfig, CqlSession};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = CqlConfig::new(["localhost:9042"])
    ///     .with_startup_retry(RetryConfig::high_reliability());
    ///
    /// let session = CqlSession::connect_with_retry(&config).await?;
    /// # let _ = session;
    /// # Ok(())
    /// # }
    /// ```
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn connect_with_retry(config: &CqlConfig) -> CqlResult<Self> {
        use crate::retry::{RetryConfig, with_retry};

        let retry_config = config.startup_retry.clone().unwrap_or_else(|| {
            RetryConfig::new()
                .max_retries(5)
                .base_delay(std::time::Duration::from_secs(1))
                .max_delay(std::time::Duration::from_secs(30))
                .jitter_factor(0.25)
        });

        debug!(
            max_retries = retry_config.max_retries,
            base_delay_ms = retry_config.base_delay.as_millis(),
            "Connecting to YCQL with startup retry"
        );

        with_retry(retry_config, || async { Self::connect(config).await }).await
    }

    /// The session serving **now** — after a credential rotation, the old one — so take it
    /// for one operation and never keep it.
    #[must_use]
    pub fn inner(&self) -> Arc<Session> {
        self.inner.load_full()
    }

    /// Serves this handle and every clone of it from `fresh`'s session from now on, returning
    /// the session it replaced; that one closes when the operations still holding it end.
    pub fn rotate_to(&self, fresh: &CqlSession) -> Arc<Session> {
        self.inner.swap(fresh.inner())
    }

    /// Returns the configuration.
    pub fn config(&self) -> &CqlConfig {
        &self.config
    }

    /// Checks if the session is healthy.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn health_check(&self) -> CqlResult<()> {
        self.inner()
            .query_unpaged("SELECT now() FROM system.local", &[])
            .await
            .map_err(|e| CqlError::Query(Box::new(e)))?;
        Ok(())
    }

    /// Returns cluster metadata.
    ///
    ///
    /// Every node is reported up: the driver removes down nodes from the metadata.
    pub fn cluster_info(&self) -> ClusterInfo {
        let metadata = self.inner().get_cluster_state();
        let nodes: Vec<NodeInfo> = metadata
            .get_nodes_info()
            .iter()
            .map(|node| {
                // The driver removes unreachable nodes from the metadata.
                NodeInfo {
                    address: node.address.to_string(),
                    datacenter: node.datacenter.clone(),
                    rack: node.rack.clone(),
                    is_up: true,
                }
            })
            .collect();

        ClusterInfo {
            nodes,
            total_nodes: metadata.get_nodes_info().len(),
        }
    }
}

/// Parses `from=to` address-rewrite pairs into the driver's translator map.
///
/// A malformed pair is an error rather than a skipped entry. The source is a socket address;
/// the target may be `host:port`, resolved at dial time ([`TranslationTarget`]).
///
/// # Errors
/// [`CqlError::Config`] if a pair is not `from=to`, if the source is not a socket
/// address, or if the target carries no port.
fn address_translation(rules: &[String]) -> CqlResult<HashMap<SocketAddr, TranslationTarget>> {
    rules
        .iter()
        .filter(|rule| !rule.trim().is_empty())
        .map(|rule| {
            let (from, to) = rule.split_once('=').ok_or_else(|| {
                CqlError::Config(format!(
                    "address translation '{rule}' is not a 'from=to' pair"
                ))
            })?;
            let source = from.trim().parse::<SocketAddr>().map_err(|error| {
                CqlError::Config(format!(
                    "address translation '{rule}': source '{from}' is not a socket address: {error}"
                ))
            })?;
            Ok((source, translation_target(to.trim(), rule)?))
        })
        .collect()
}

/// Reads a rewrite target, which is either a fixed address or a name to resolve.
///
/// # Errors
/// [`CqlError::Config`] if the target has no `:port`, or the port is not a number.
fn translation_target(target: &str, rule: &str) -> CqlResult<TranslationTarget> {
    if let Ok(address) = target.parse::<SocketAddr>() {
        return Ok(TranslationTarget::Fixed(address));
    }

    // `rsplit_once`, so an IPv6 literal's own colons stay with the host.
    let (host, port) = target.rsplit_once(':').ok_or_else(|| {
        CqlError::Config(format!(
            "address translation '{rule}': target '{target}' has no port — a target is \
             'address:port' or 'host:port'"
        ))
    })?;
    let port = port.parse::<u16>().map_err(|error| {
        CqlError::Config(format!(
            "address translation '{rule}': target port '{port}' is not a port number: {error}"
        ))
    })?;
    if host.is_empty() {
        return Err(CqlError::Config(format!(
            "address translation '{rule}': target '{target}' has no host"
        )));
    }
    Ok(TranslationTarget::Resolved {
        host: host.trim_matches(['[', ']']).to_owned(),
        port,
    })
}

/// Builds a rustls [`ClientConfig`](rustls::ClientConfig) from the crate's [`TlsConfig`].
///
/// `accept_invalid_certs` (debug builds only) disables server verification, never the client
/// identity.
fn build_tls_config(tls: &TlsConfig) -> CqlResult<rustls::ClientConfig> {
    // Idempotent: an already-installed provider makes this a no-op.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let client_auth = client_auth_material(tls)?;

    #[cfg(debug_assertions)]
    if tls.accept_invalid_certs {
        let builder = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertVerification));
        return match client_auth {
            Some((chain, key)) => builder
                .with_client_auth_cert(chain, key)
                .map_err(|e| CqlError::Config(format!("client certificate rejected: {e}"))),
            None => Ok(builder.with_no_client_auth()),
        };
    }

    let mut roots = rustls::RootCertStore::empty();
    let pem: Option<Vec<u8>> = match (&tls.ca_cert_path, &tls.ca_cert_pem) {
        (Some(path), _) => Some(
            std::fs::read(path)
                .map_err(|e| CqlError::Config(format!("reading CA cert {path}: {e}")))?,
        ),
        (None, Some(pem)) => Some(pem.clone().into_bytes()),
        (None, None) => None,
    };
    if let Some(bytes) = pem {
        for cert in CertificateDer::pem_slice_iter(&bytes) {
            let cert = cert.map_err(|e| CqlError::Config(format!("parsing CA cert: {e}")))?;
            roots
                .add(cert)
                .map_err(|e| CqlError::Config(format!("adding CA cert: {e}")))?;
        }
    }

    // A configured name is verified instead of the dialled address, which the driver derives and
    // offers no way to influence; see `PinnedServerName`.
    let builder = match &tls.server_name {
        Some(name) => {
            let pinned = rustls::pki_types::ServerName::try_from(name.clone()).map_err(|e| {
                CqlError::Config(format!("TLS server name '{name}' is not a valid name: {e}"))
            })?;
            let inner = rustls::client::WebPkiServerVerifier::builder(Arc::new(roots))
                .build()
                .map_err(|e| {
                    CqlError::Config(format!(
                        "cannot verify against '{name}': no usable trust anchor — a server \
                         name pins which name is checked, it does not supply the CA that \
                         signs it: {e}"
                    ))
                })?;
            debug!(server_name = %name, "verifying the cluster by name rather than by address");
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(PinnedServerName::new(pinned, inner)))
        }
        None => rustls::ClientConfig::builder().with_root_certificates(roots),
    };
    match client_auth {
        Some((chain, key)) => builder
            .with_client_auth_cert(chain, key)
            .map_err(|e| CqlError::Config(format!("client certificate rejected: {e}"))),
        None => Ok(builder.with_no_client_auth()),
    }
}

/// Reads the configured client certificate chain and private key, if any.
///
/// `None` when no client identity is configured; a half-configured pair is none.
type ClientAuth = (
    Vec<rustls::pki_types::CertificateDer<'static>>,
    rustls::pki_types::PrivateKeyDer<'static>,
);

fn client_auth_material(tls: &TlsConfig) -> CqlResult<Option<ClientAuth>> {
    if !tls.has_client_cert() {
        return Ok(None);
    }

    // Inline PEM wins over a path, as for the CA anchor.
    let read = |pem: &Option<String>, path: &Option<String>, what: &str| -> CqlResult<Vec<u8>> {
        match (pem, path) {
            (Some(pem), _) => Ok(pem.clone().into_bytes()),
            (None, Some(path)) => std::fs::read(path)
                .map_err(|e| CqlError::Config(format!("reading {what} {path}: {e}"))),
            (None, None) => Err(CqlError::Config(format!("no {what} configured"))),
        }
    };

    let cert_bytes = read(&tls.client_cert_pem, &tls.client_cert_path, "client cert")?;
    let key_bytes = read(&tls.client_key_pem, &tls.client_key_path, "client key")?;

    let chain = CertificateDer::pem_slice_iter(&cert_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CqlError::Config(format!("parsing client cert: {e}")))?;
    if chain.is_empty() {
        return Err(CqlError::Config(
            "client certificate contains no certificates".to_string(),
        ));
    }

    let key = PrivateKeyDer::from_pem_slice(&key_bytes).map_err(|e| match e {
        PemError::NoItemsFound => {
            CqlError::Config("client key contains no private key".to_string())
        }
        other => CqlError::Config(format!("parsing client key: {other}")),
    })?;

    Ok(Some((chain, key)))
}

/// A certificate verifier that accepts any server certificate.
///
/// Debug builds only, when `TlsConfig::accept_invalid_certs` is set.
#[cfg(debug_assertions)]
#[derive(Debug)]
struct NoCertVerification;

#[cfg(debug_assertions)]
impl rustls::client::danger::ServerCertVerifier for NoCertVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::aws_lc_rs::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod client_auth_tests {
    use super::*;

    /// A throwaway self-signed pair that proves PEM parsing and guards nothing.
    const TEST_CERT: &str = "-----BEGIN CERTIFICATE-----\nMIIDGTCCAgGgAwIBAgIUGoUDx0c6Pxaf1jdZva7hMd4qVBUwDQYJKoZIhvcNAQEL\nBQAwGzEZMBcGA1UEAwwQZnVuZGFtZW50YWwtdGVzdDAgFw0yNjA3MjcwODA1MDRa\nGA8yMTI2MDcwMzA4MDUwNFowGzEZMBcGA1UEAwwQZnVuZGFtZW50YWwtdGVzdDCC\nASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAMNCHPRTwg9s3ZYtd4G9DPAL\nCR6pG/nSlSvsbZTZM5T1EKbzhuM3V1l5/zpjSEJtKj6VLtfT1qR1oeLFlElDQaxv\nGmFzOrx8/rBLVD+yPVkB+6zKQi4icm2+SkRj7FB3s4h+RFeRHONzk7mEmtbLXcjt\nOkRezhg2Kfmkk4oRYorUsu+kXSj1p6Czub3N51fYW2ygdTIlbrtY1EAj9hrArsTl\nDOwVXts1h/n68bapWAhvoRnNfctKdKuVN+W4niTgbYJny/ezGaXTs3BYhn8E2Q96\nYARlOOtIuBjbxV6iFElhQvxkmhAFMKBnHiReyw8PNY0nMSX359RPE7u1FKMZBbcC\nAwEAAaNTMFEwHQYDVR0OBBYEFB3b3otjB83LS7BINW4cMUuYTVU9MB8GA1UdIwQY\nMBaAFB3b3otjB83LS7BINW4cMUuYTVU9MA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZI\nhvcNAQELBQADggEBAIkqGvywEpL1sz1iPN9BfL1gfGU/qpw6/IYVQqoyCiParaIn\np0IGui+JI8Tpnzgl1mla0hDn+7K2XH3cAvQN4aHrgYgiNI19PGBScq+YwNAsQ0bz\ngKjFpMeKHzQRrb+Rt2ZGAtFJuuJ7ZYpg8z3c7hh/d8Bbwixz4jdia3kKLlbjjYUA\n4GwlB8Tctd8D4oObxKOMQ6b2+00NuyRxTD16RVXqSat9w//D3QI/UiJ0ghq94OMv\nu0cffmbSd+fKv/FXKeKm7ZfT/sVBgHzfFjYZmKH+FiRU4mz/LtNuoxpWidxl6vc8\nkgthEyyhdt8m4NRwuUehkxZA8eShpOVZVYNTUug=\n-----END CERTIFICATE-----";
    const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIIEuwIBADANBgkqhkiG9w0BAQEFAASCBKUwggShAgEAAoIBAQDDQhz0U8IPbN2W\nLXeBvQzwCwkeqRv50pUr7G2U2TOU9RCm84bjN1dZef86Y0hCbSo+lS7X09akdaHi\nxZRJQ0Gsbxphczq8fP6wS1Q/sj1ZAfusykIuInJtvkpEY+xQd7OIfkRXkRzjc5O5\nhJrWy13I7TpEXs4YNin5pJOKEWKK1LLvpF0o9aegs7m9zedX2FtsoHUyJW67WNRA\nI/YawK7E5QzsFV7bNYf5+vG2qVgIb6EZzX3LSnSrlTfluJ4k4G2CZ8v3sxml07Nw\nWIZ/BNkPemAEZTjrSLgY28VeohRJYUL8ZJoQBTCgZx4kXssPDzWNJzEl9+fUTxO7\ntRSjGQW3AgMBAAECgf8Bf+9rShzZLJnMDRJhFNtf7VEVWD9NyeTTSaEAh76kf+dv\nyDAHkVopwbgsAOfLubXtARAki3QZ+RVazyAK4y9Wt6VRvUCRws40rS/LkpSiScCa\nONxDIXyYJzrPp1iNK1eGgvLvzinfl8iHfKrbBrw7YhTKaRf82p3c4KT7uH5cwpUN\nlknRl08UHwWWMMtF4+fC6DGgvNwq/GdRKErhslMBmHjAfMWiKYBV0SsHyI8Ct/Xi\nTT/NFzYr9JCldY/yZR6WSseorgNCtxz2p/K7HUbW2ihjqgG8myxJIcNA0mz0Vty1\nl2csfeotJBEdTGPlSsoKqLcq4VnMt0viQVNLLWkCgYEA7PeTFRw3mLEO3pggVO5X\noDjp8g7dvgWAYCAW0KMUYmGPF6PRwHY/A3nFqTz+NKvMQ+bgTO5NeSRJdBoU7Upv\nsPEpdnj1khPEb2OXDkH/kPZoLn1XyK35WKZqtAsf2C84KG8BKprTZ8do8H84zALe\ngwGBrIPm6EneGTv6o4OpJ0kCgYEA0vDwC+eBFusvYJlobD9ykcmbWuvVS4/Wh6IX\nfqHqkobE5GKq9oBKc/jCSbZtBVz0oABDjLtDtVyIjud40hhXi3uPuSuCbnckJhrW\nRS8MyK8K+EQAl4Y5o6Sr27uSqJqx10u0HX/k5rQTRavUYqsaAKgLMLltrnSwgfhT\nSPxPxP8CgYEAhN2DwJBjs9+C4NgXOMeypgmDUs93ETZwdnVHlY/6u/bLjoux7eHf\nDwnWBEBIoCu7ik8HcNsk4UmxTcHZ/fqAiYuOnfjQaClhOLt0VPA4ZYVjgL2S3Nh/\nOG+G4JBkOIixSiqDUuogos2yTzZmGC17qaTMmngSnifcufVhYRFPZNECgYBzQ2q3\n5ol4X4zk6Q7RT54oIZT9iq5jeraMKLtT2+AUrwW5kly/E30bf8uSgddyvceCAoLm\nzJED8gtAoyU3EEwOELeJREgiWoHrPHDnHv2KT3PcEzSLl7G5RMsP56AM93LEJT7H\ne9I1aiK5yFnmjH5J64kp17kiQesNKgdAc3aXOwKBgDRRLrBjFSdBcdZtPyfogiLX\nejWmtNZ0t6eZ1h26qPNrOnrG05NmdjSY+ouH21l9eVWFuzWOrpJEKUlW/bcxSfvx\nWj4tmcxy3Fu7bHC0/y7wdVxjfgC52FiWz7PO6iuKSD6ArU2bZPd6+M1RctF4DKRt\nGDtugciY4NGzHVec0Ush\n-----END PRIVATE KEY-----";

    fn tls_with_client_pem() -> TlsConfig {
        TlsConfig::new().client_cert_pem(TEST_CERT, TEST_KEY)
    }

    #[test]
    fn a_configured_client_identity_is_parsed() {
        let (chain, _key) = client_auth_material(&tls_with_client_pem())
            .expect("the pair parses")
            .expect("a configured pair yields an identity");

        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn a_configured_client_identity_reaches_the_rustls_config() {
        assert!(build_tls_config(&tls_with_client_pem()).is_ok());
    }

    #[test]
    fn a_pair_missing_its_key_or_its_certificate_is_refused() {
        let no_key = client_auth_material(&TlsConfig::new().client_cert_pem(TEST_CERT, TEST_CERT))
            .expect_err("a certificate is not a key");
        assert!(
            no_key
                .to_string()
                .contains("client key contains no private key"),
            "{no_key}"
        );

        let no_cert = client_auth_material(&TlsConfig::new().client_cert_pem(TEST_KEY, TEST_KEY))
            .expect_err("a key is not a certificate");
        assert!(
            no_cert
                .to_string()
                .contains("client certificate contains no certificates"),
            "{no_cert}"
        );
    }

    #[test]
    fn no_client_identity_configured_is_none() {
        assert!(
            client_auth_material(&TlsConfig::new())
                .expect("absent is not an error")
                .is_none()
        );
    }

    #[test]
    fn half_a_pair_is_no_identity() {
        let mut tls = TlsConfig::new();
        tls.client_cert_pem = Some(TEST_CERT.to_string());

        assert!(
            client_auth_material(&tls)
                .expect("half a pair is not an error")
                .is_none()
        );
    }

    #[test]
    fn a_malformed_key_is_a_config_error() {
        let tls =
            TlsConfig::new().client_cert_pem(TEST_CERT, "-----BEGIN PRIVATE KEY-----\nnope\n");

        assert!(client_auth_material(&tls).is_err());
    }
}

#[cfg(test)]
mod address_translation_tests {
    use super::*;

    #[test]
    fn a_pair_is_parsed_into_a_rewrite() {
        let rules = ["192.168.97.2:9142=127.0.0.1:9142".to_string()];

        let map = address_translation(&rules).expect("a well-formed pair parses");

        assert_eq!(
            map.get(&"192.168.97.2:9142".parse::<SocketAddr>().expect("addr")),
            Some(&TranslationTarget::Fixed(
                "127.0.0.1:9142".parse::<SocketAddr>().expect("addr")
            ))
        );
    }

    /// Resolved at dial time, so the rule outlives a container recreate.
    #[test]
    fn a_named_target_is_kept_for_dial_time() {
        let rules = ["127.0.0.1:9142=scylla:9142".to_string()];

        let map = address_translation(&rules).expect("a named target parses");

        assert_eq!(
            map.get(&"127.0.0.1:9142".parse::<SocketAddr>().expect("addr")),
            Some(&TranslationTarget::Resolved {
                host: "scylla".to_owned(),
                port: 9142,
            })
        );
    }

    #[test]
    fn an_ipv6_target_is_fixed_not_mistaken_for_a_name() {
        let rules = ["127.0.0.1:9142=[::1]:9142".to_string()];

        let map = address_translation(&rules).expect("an IPv6 target parses");

        assert_eq!(
            map.get(&"127.0.0.1:9142".parse::<SocketAddr>().expect("addr")),
            Some(&TranslationTarget::Fixed(
                "[::1]:9142".parse::<SocketAddr>().expect("addr")
            ))
        );
    }

    #[test]
    fn a_target_without_a_port_is_an_error() {
        assert!(address_translation(&["127.0.0.1:9142=scylla".to_string()]).is_err());
        assert!(address_translation(&["127.0.0.1:9142=scylla:".to_string()]).is_err());
        assert!(address_translation(&["127.0.0.1:9142=:9142".to_string()]).is_err());
    }

    /// `${VAR:-}` interpolation spells "not configured" as an empty entry.
    #[test]
    fn an_empty_entry_is_no_rule() {
        let rules = ["".to_string(), "   ".to_string()];

        assert!(
            address_translation(&rules)
                .expect("empty entries are not an error")
                .is_empty()
        );
    }

    #[test]
    fn a_malformed_rule_is_an_error_not_a_skip() {
        assert!(address_translation(&["not-a-pair".to_string()]).is_err());
        assert!(address_translation(&["nope=127.0.0.1:9142".to_string()]).is_err());
        assert!(address_translation(&["127.0.0.1:9142=nope".to_string()]).is_err());
    }
}
