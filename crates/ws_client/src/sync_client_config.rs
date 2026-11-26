//! [`SyncClientConfig`] — how a [`SyncClient`](crate::SyncClient) connects and stays stable.

use std::time::Duration;

use fundamental_core::secrets::SecretString;

use crate::{ReconnectMaxAttemptsIsRemoved, SyncAuth};

/// Configuration for a [`SyncClient`](crate::SyncClient): where to connect, how to
/// authenticate, and how often to reconcile and probe. There is no knob for giving up.
#[derive(Clone)]
pub struct SyncClientConfig {
    /// The sync server URL (`ws://…` or `wss://…`).
    pub url: String,
    /// How the connection authenticates on its first frame.
    pub auth: SyncAuth,
    /// How often to reconcile against the durable pull cursor while connected; `None`
    /// reconciles only at the [`liveness_interval`](Self::liveness_interval).
    pub reconcile_interval: Option<Duration>,
    /// The longest a live session goes without a pull, its liveness probe. Default 60 s.
    pub liveness_interval: Duration,
    /// How long each request waits for its reply before the connection is lost. Default 30 s.
    pub request_timeout: Duration,
    /// Whether to open a live `slice.subscribe`; `false` runs poll-only, for a hub without one.
    pub live_subscribe: bool,
}

impl SyncClientConfig {
    /// Config for `url` with a JWT/session `token`, reconciling every 30 seconds.
    #[must_use]
    pub fn new(url: impl Into<String>, token: SecretString) -> Self {
        Self::with_auth(url, SyncAuth::Jwt(token))
    }

    /// Config for `url` with a blinded-owner vault connection authenticated by `owner`.
    #[cfg(feature = "vault-auth")]
    #[must_use]
    pub fn vault(
        url: impl Into<String>,
        owner: std::sync::Arc<fundamental_keys::VaultOwnerKey>,
    ) -> Self {
        Self::with_auth(url, SyncAuth::Vault(owner))
    }

    /// Config for `url` with an explicit [`SyncAuth`], reconciling every 30 seconds.
    #[must_use]
    pub fn with_auth(url: impl Into<String>, auth: SyncAuth) -> Self {
        Self {
            url: url.into(),
            auth,
            reconcile_interval: Some(Duration::from_secs(30)),
            liveness_interval: Duration::from_secs(60),
            request_timeout: Duration::from_secs(30),
            live_subscribe: true,
        }
    }

    /// Removed: no argument compiles, because a stop state the network can reach lets anyone
    /// end a device's sync by failing its connects.
    #[must_use]
    pub fn with_reconnect_max_attempts<Removed: ReconnectMaxAttemptsIsRemoved>(
        self,
        _attempts: Removed,
    ) -> Self {
        self
    }

    /// Sets the periodic reconcile interval (`None` leaves only the liveness probe).
    #[must_use]
    pub fn with_reconcile_interval(mut self, interval: Option<Duration>) -> Self {
        self.reconcile_interval = interval;
        self
    }

    /// Sets the longest a live session goes without a pull.
    #[must_use]
    pub fn with_liveness_interval(mut self, interval: Duration) -> Self {
        self.liveness_interval = interval;
        self
    }

    /// Sets how long each request waits for its reply.
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Its reconcile interval, never longer than the liveness interval.
    pub(crate) fn probe_interval(&self) -> Duration {
        self.reconcile_interval
            .map_or(self.liveness_interval, |interval| {
                interval.min(self.liveness_interval)
            })
    }

    /// Sets whether to open a live subscription (`false` = poll-only).
    #[must_use]
    pub fn with_live_subscribe(mut self, live_subscribe: bool) -> Self {
        self.live_subscribe = live_subscribe;
        self
    }
}

impl std::fmt::Debug for SyncClientConfig {
    /// Never renders the auth material ([`SyncAuth`]'s `Debug` redacts it).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncClientConfig")
            .field("url", &self.url)
            .field("auth", &self.auth)
            .field("reconcile_interval", &self.reconcile_interval)
            .field("liveness_interval", &self.liveness_interval)
            .field("request_timeout", &self.request_timeout)
            .field("live_subscribe", &self.live_subscribe)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_reconcile_periodically() {
        let config = SyncClientConfig::new("ws://localhost:9100", SecretString::new("t".into()));
        assert_eq!(config.reconcile_interval, Some(Duration::from_secs(30)));
        assert!(config.live_subscribe, "live subscription is on by default");
        assert_eq!(config.liveness_interval, Duration::from_secs(60));
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        assert_eq!(config.probe_interval(), Duration::from_secs(30));
    }

    #[test]
    fn no_reconcile_interval_still_probes_liveness() {
        let config = SyncClientConfig::new("ws://x", SecretString::new("t".into()))
            .with_reconcile_interval(None);
        assert_eq!(config.probe_interval(), Duration::from_secs(60));
    }

    #[test]
    fn a_reconcile_interval_never_outlasts_the_liveness_probe() {
        let config = SyncClientConfig::new("ws://x", SecretString::new("t".into()))
            .with_reconcile_interval(Some(Duration::from_secs(600)))
            .with_liveness_interval(Duration::from_secs(45));
        assert_eq!(config.probe_interval(), Duration::from_secs(45));
    }

    #[test]
    fn poll_only_disables_the_live_subscription() {
        let config = SyncClientConfig::new("ws://x", SecretString::new("t".into()))
            .with_live_subscribe(false);
        assert!(!config.live_subscribe);
    }

    #[test]
    fn builders_override_the_stability_knobs() {
        let config = SyncClientConfig::new("ws://x", SecretString::new("t".into()))
            .with_reconcile_interval(None);
        assert_eq!(config.reconcile_interval, None);
    }

    #[test]
    fn debug_never_renders_the_token() {
        let config = SyncClientConfig::new("ws://x", SecretString::new("super-secret".into()));
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("super-secret"));
    }
}
