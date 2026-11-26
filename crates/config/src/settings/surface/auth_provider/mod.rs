//! External auth provider settings for the Ory stack ([`AuthProviderSettings`]).

mod hydra;
mod keto;
mod kratos;
mod oathkeeper;

pub use hydra::HydraSettings;
pub use keto::KetoSettings;
pub use kratos::KratosSettings;
pub use oathkeeper::OathkeeperSettings;

use serde::{Deserialize, Serialize};

/// Configuration for external auth providers, each optional.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthProviderSettings {
    /// Ory Kratos identity management configuration.
    pub kratos: Option<KratosSettings>,
    /// Ory Hydra OAuth2/OIDC server configuration.
    pub hydra: Option<HydraSettings>,
    /// Ory Keto fine-grained authorization (Zanzibar) configuration.
    pub keto: Option<KetoSettings>,
    /// Ory Oathkeeper API gateway configuration.
    pub oathkeeper: Option<OathkeeperSettings>,
}
