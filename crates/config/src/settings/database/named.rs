//! Named database connections for multi-domain routing ([`NamedDatabaseSettings`]).

use super::{CqlSettings, SqlSettings};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Named database connections, keyed by the domain `#[model(domain = "…")]` routes to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NamedDatabaseSettings {
    /// Named YSQL connections by domain.
    #[serde(default)]
    pub sql: HashMap<String, SqlSettings>,
    /// Named YCQL connections by domain.
    #[serde(default)]
    pub cql: HashMap<String, CqlSettings>,
}
