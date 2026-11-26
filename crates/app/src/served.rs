use fundamental_config::AppSettings;
use fundamental_core::Environment;
use fundamental_sql::{SqlConnections, SqlPool, SqlResult};

/// What an [`App`](crate::App) is served with: its settings, its database pools and its listed pages.
#[derive(Debug, Clone)]
pub struct Served {
    settings: AppSettings,
    connections: SqlConnections,
    listed_pages: Vec<String>,
}

impl Served {
    /// What an app is served with.
    #[must_use]
    pub fn new(
        settings: AppSettings,
        connections: SqlConnections,
        listed_pages: Vec<String>,
    ) -> Self {
        Self {
            settings,
            connections,
            listed_pages,
        }
    }

    /// The app's settings, as loaded for this process.
    #[must_use]
    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// The environment the app runs in.
    #[must_use]
    pub fn environment(&self) -> Environment {
        self.settings.environment()
    }

    /// Every database pool the app declares.
    #[must_use]
    pub fn connections(&self) -> &SqlConnections {
        &self.connections
    }

    /// The pool for `domain`; `"default"` is the app's `[database.sql]`.
    ///
    /// # Errors
    /// [`SqlError::Config`](fundamental_sql::SqlError::Config) when the app declares no pool
    /// for `domain`.
    pub fn pool(&self, domain: &str) -> SqlResult<SqlPool> {
        self.connections.pool_for_domain(domain)
    }

    /// The app's own pages a search engine may list, at their canonical paths.
    #[must_use]
    pub fn listed_pages(&self) -> &[String] {
        &self.listed_pages
    }
}
