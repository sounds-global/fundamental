//! Messaging configuration settings ([`MessagingSettings`]).

mod nats;
mod nats_auth;
mod nkey_public_key;
mod nkey_signer;

pub use nats::NatsSettings;
pub use nats_auth::NatsAuth;
pub use nkey_public_key::NkeyPublicKey;
pub use nkey_signer::NkeySigner;

use serde::{Deserialize, Serialize};

/// Messaging configuration settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessagingSettings {
    /// NATS configuration.
    pub nats: Option<NatsSettings>,
}
