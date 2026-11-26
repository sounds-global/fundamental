use std::path::PathBuf;
use std::sync::Arc;

use fundamental_core::secrets::SecretString;
use fundamental_offline::db::{OfflineConfig, OfflineLocation};
use fundamental_ws_client::SyncClientConfig;

use crate::{NativeError, TokenSource};

/// Configuration for the [`SyncHost`](crate::SyncHost): the sync server URL,
/// an optional [`TokenSource`], and where the durable database file lives.
///
/// The database defaults to `sync.db` in the app data directory; the token source is resolved
/// once when the host starts, and without one sessions connect unauthenticated.
#[derive(Clone)]
#[must_use = "a SyncHostConfig does nothing until handed to the sync host"]
pub struct SyncHostConfig {
    url: String,
    token: Option<SecretString>,
    token_source: Option<Arc<dyn TokenSource>>,
    database_path: Option<PathBuf>,
    database_name: String,
}

impl SyncHostConfig {
    /// Config for the sync server at `url`, unauthenticated, database in the app data directory.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            token: None,
            token_source: None,
            database_path: None,
            database_name: "sync.db".to_owned(),
        }
    }

    /// Sets a fixed session token (a dev token from the environment, say).
    ///
    /// Wins over [`with_token_source`](Self::with_token_source); prefer the source for anything
    /// that rotates.
    pub fn with_token(mut self, token: SecretString) -> Self {
        self.token = Some(token);
        self
    }

    /// Sets the token source sessions authenticate with.
    pub fn with_token_source(mut self, source: impl TokenSource) -> Self {
        self.token_source = Some(Arc::new(source));
        self
    }

    /// Puts the database at an explicit file path instead of the app data directory.
    pub fn with_database_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.database_path = Some(path.into());
        self
    }

    /// Renames the database file inside the app data directory.
    pub fn with_database_name(mut self, name: impl Into<String>) -> Self {
        self.database_name = name.into();
        self
    }

    /// Resolves the offline configuration against `app_data_dir` unless a path was set.
    pub(crate) fn offline_config(&self, app_data_dir: &std::path::Path) -> OfflineConfig {
        let path = self
            .database_path
            .clone()
            .unwrap_or_else(|| app_data_dir.join(&self.database_name));
        OfflineConfig {
            location: OfflineLocation::Persistent(path.to_string_lossy().into_owned()),
            ..OfflineConfig::memory()
        }
    }

    /// Resolves the session configuration, querying the token source once.
    ///
    /// # Errors
    /// [`NativeError::Token`] when the token source fails; the host then does not start.
    pub(crate) async fn sync_config(&self) -> Result<SyncClientConfig, NativeError> {
        let token = match (&self.token, &self.token_source) {
            (Some(token), _) => token.clone(),
            (None, Some(source)) => source
                .token()
                .await
                .map_err(|source| NativeError::Token { source })?
                .unwrap_or_else(|| SecretString::new(String::new())),
            (None, None) => SecretString::new(String::new()),
        };
        Ok(SyncClientConfig::new(self.url.clone(), token))
    }
}

impl std::fmt::Debug for SyncHostConfig {
    /// Never renders the token source.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncHostConfig")
            .field("url", &self.url)
            .field("has_token", &self.token.is_some())
            .field("has_token_source", &self.token_source.is_some())
            .field("database_path", &self.database_path)
            .field("database_name", &self.database_name)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::SyncHostConfig;
    use fundamental_offline::db::OfflineLocation;
    use fundamental_ws_client::SyncAuth;

    #[test]
    fn database_defaults_into_the_app_data_dir() {
        let config = SyncHostConfig::new("ws://localhost:9100");
        let offline = config.offline_config(Path::new("/data/app"));
        assert_eq!(
            offline.location,
            OfflineLocation::Persistent(
                Path::new("/data/app")
                    .join("sync.db")
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }

    #[test]
    fn an_explicit_path_wins_over_the_data_dir() {
        let config = SyncHostConfig::new("ws://x").with_database_path("/tmp/elsewhere/mirror.db");
        let offline = config.offline_config(Path::new("/data/app"));
        assert_eq!(
            offline.location,
            OfflineLocation::Persistent("/tmp/elsewhere/mirror.db".to_owned())
        );
    }

    #[test]
    fn the_database_name_is_overridable() {
        let config = SyncHostConfig::new("ws://x").with_database_name("mirror.db");
        let offline = config.offline_config(Path::new("/data/app"));
        assert_eq!(
            offline.location,
            OfflineLocation::Persistent(
                Path::new("/data/app")
                    .join("mirror.db")
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }

    #[tokio::test]
    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    async fn without_a_token_source_sessions_connect_unauthenticated() {
        let config = SyncHostConfig::new("ws://localhost:9100");
        let sync = config.sync_config().await.expect("resolves");
        assert_eq!(sync.url, "ws://localhost:9100");
        // No token source still authenticates as JWT with an empty credential; the catch-all
        // is reachable only under `vault-auth`.
        match &sync.auth {
            SyncAuth::Jwt(token) => assert!(token.expose().is_empty()),
            #[allow(unreachable_patterns)]
            other => panic!("expected JWT auth, got {other:?}"),
        }
    }

    #[tokio::test]
    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    async fn a_fixed_token_wins_and_never_renders_in_debug() {
        use fundamental_core::secrets::SecretString;

        let config = SyncHostConfig::new("ws://x").with_token(SecretString::new("t0p".into()));
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("t0p"), "{rendered}");

        let sync = config.sync_config().await.expect("resolves");
        match &sync.auth {
            SyncAuth::Jwt(token) => assert_eq!(token.expose(), "t0p"),
            #[allow(unreachable_patterns)]
            other => panic!("expected JWT auth, got {other:?}"),
        }
    }
}
