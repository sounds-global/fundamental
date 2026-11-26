mod composite;
mod configured;
mod env;
mod envelope;
mod local;
mod matches_declared_fingerprint;

pub use composite::CompositeKeySource;
pub use configured::{ConfiguredKey, ConfiguredKeySource};
pub use env::EnvKeySource;
pub use envelope::EnvelopeKeySource;
// Gated like the item it re-exports.
#[cfg(debug_assertions)]
pub use local::LocalKeySource;

use crate::{KeyConfig, KeyResult};

/// Loads keys from an external source.
#[async_trait::async_trait]
pub trait KeySource: Send + Sync {
    /// Loads keys from the source.
    async fn load_keys(&self) -> KeyResult<Vec<KeyConfig>>;

    /// Reloads keys, for rotation.
    async fn refresh(&self) -> KeyResult<Vec<KeyConfig>> {
        self.load_keys().await
    }
}
