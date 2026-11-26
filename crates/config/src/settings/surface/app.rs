//! App settings ([`AppConfig`]).

use std::collections::HashMap;

use super::{
    AuthSettings, ClientSettings, CustomSettings, ObservabilitySettings, SurfaceEntryConfig,
};
use crate::settings::{AuthzSettings, DatabaseSettings, MessagingSettings};

/// A named app: its surfaces and the sections the loader promotes when it is active.
#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    /// Surfaces in this app, keyed by surface type.
    pub surfaces: HashMap<String, SurfaceEntryConfig>,
    /// App-scoped database configuration.
    pub database: Option<DatabaseSettings>,
    /// App-scoped messaging configuration.
    pub messaging: Option<MessagingSettings>,
    /// App-scoped auth configuration.
    pub auth: Option<AuthSettings>,
    /// App-scoped authorization-engine configuration.
    pub authz: Option<AuthzSettings>,
    /// App-scoped client configuration.
    pub client: Option<ClientSettings>,
    /// App-scoped observability configuration.
    pub observability: Option<ObservabilitySettings>,
    /// App-scoped custom configuration.
    pub custom: Option<CustomSettings>,
    /// The code of the brand this app's deployment wears, checked at boot.
    pub brand: Option<String>,
    /// Sections and leaves carrying this app's identity, which an instance must restate.
    pub instance_identity: Vec<String>,
}
