//! Where a surface's listener comes from ([`ListenMode`]).

use serde::{Deserialize, Serialize};

/// Where a surface's listener comes from.
///
/// ```toml
/// web = { listen = "activated", tls = "terminated_upstream" }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListenMode {
    /// The surface binds its own `host` and `port`.
    #[default]
    Tcp,
    /// The surface serves the socket passed to the process under its own name.
    Activated,
}
