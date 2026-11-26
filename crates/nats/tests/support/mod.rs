//! What the secret-store suites share: the OpenBao, a client of it, a readable log, waits.

#![allow(dead_code, reason = "each suite uses the part of this it needs")]

use std::time::Duration;

use fundamental_config::settings::{NatsSettings, VaultAuth, VaultSettings};
use fundamental_core::secrets::InMemorySecret;
use fundamental_observability::testing::CapturedLogs;
use fundamental_secrets::VaultClient;
use fundamental_test_containers::container_name::OPENBAO_03_NATS;
use fundamental_test_containers::openbao::OpenBaoServer;
use tracing::subscriber::DefaultGuard;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::SubscriberExt;

/// The Transit mount the NKey suites sign under.
pub const TRANSIT: &str = "transit";

/// The development OpenBao, with the `acme` key-value mount and a Transit engine.
pub async fn openbao() -> &'static OpenBaoServer {
    let server = OpenBaoServer::shared(OPENBAO_03_NATS)
        .await
        .expect("the OpenBao test server");
    server.enable_kv("acme").await.expect("the acme mount");
    server
        .enable_transit(TRANSIT)
        .await
        .expect("the transit mount");
    server
}

/// A client of the secret store at `address`, logged in with the development root token.
pub async fn vault_at(address: &str) -> VaultClient {
    VaultClient::from_settings(&VaultSettings {
        enabled: true,
        address: address.to_owned(),
        auth: VaultAuth::Token {
            token: InMemorySecret::new("fundamental-test-root".to_owned()),
        },
        ..VaultSettings::default()
    })
    .await
    .expect("the client logs in")
    .expect("an enabled backend builds a client")
}

/// A path no other test, and no earlier run, has written.
pub fn unique(name: &str) -> String {
    format!("nats/{name}-{}", uuid::Uuid::new_v4().simple())
}

/// `[messaging.nats]` as the loader reads it.
pub fn nats_settings(toml: &str) -> NatsSettings {
    toml::from_str(toml).expect("the section parses")
}

/// Every event this thread's subscriber sees, read back as text.
#[derive(Clone, Default)]
pub struct CapturedLog(CapturedLogs);

impl CapturedLog {
    /// Captures this thread's events at `trace`, except `async_nats::connection` at `debug`, as
    /// the framework's subscriber holds it.
    pub fn install() -> (Self, DefaultGuard) {
        let captured = CapturedLogs::default();
        let subscriber = tracing_subscriber::registry().with(
            captured.layer().with_filter(
                Targets::new()
                    .with_default(LevelFilter::TRACE)
                    .with_target("async_nats::connection", LevelFilter::DEBUG),
            ),
        );
        let guard = tracing::subscriber::set_default(subscriber);
        (Self(captured), guard)
    }

    /// Everything captured so far.
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// Waits until the captured log contains `needle`, or panics naming it after `budget`.
    pub async fn wait_for(&self, needle: &str, budget: Duration) {
        let deadline = tokio::time::Instant::now() + budget;
        while !self.text().contains(needle) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the log never said {needle:?} within {budget:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// How many times `needle` appears.
    pub fn count(&self, needle: &str) -> usize {
        self.text().matches(needle).count()
    }
}

/// Waits until `condition` holds, or panics naming `what` after `budget`.
pub async fn eventually(what: &str, budget: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + budget;
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what} did not happen within {budget:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
