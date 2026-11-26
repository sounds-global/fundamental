use fundamental_core::env::EnvVars;
use tracing::{instrument, warn};

use crate::{EncryptionAlgorithm, KeyConfig, KeyError, KeyResult, KeySource};

/// A key source that loads keys from environment variables.
///
/// Reads `{PREFIX}_{ID}` (base64 material) and `{PREFIX}_{ID}_VERSION` (optional, default 1).
///
/// ```no_run
/// # async fn run() -> fundamental_encryption::KeyResult<()> {
/// use fundamental_encryption::{EnvKeySource, KeyManagerBuilder};
///
/// let source = EnvKeySource::new(vec!["default".into(), "pii".into()]);
/// let key_manager = KeyManagerBuilder::from_source(&source).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct EnvKeySource {
    prefix: String,
    key_ids: Vec<String>,
    algorithm: EncryptionAlgorithm,
    /// Where the key material is read from; the process unless a caller says otherwise.
    vars: EnvVars,
}

impl EnvKeySource {
    /// Creates a new environment variable key source.
    pub fn new(key_ids: Vec<String>) -> Self {
        Self {
            prefix: "FUNDAMENTAL_KEY".to_string(),
            key_ids,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            vars: EnvVars::process(),
        }
    }

    /// Creates with a custom prefix.
    pub fn with_prefix(prefix: impl Into<String>, key_ids: Vec<String>) -> Self {
        Self {
            prefix: prefix.into(),
            key_ids,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            vars: EnvVars::process(),
        }
    }

    /// Reads key material from `vars` rather than the process environment.
    #[must_use]
    pub fn with_vars(mut self, vars: EnvVars) -> Self {
        self.vars = vars;
        self
    }

    /// Sets the algorithm for all keys from this source.
    pub fn with_algorithm(mut self, algorithm: EncryptionAlgorithm) -> Self {
        self.algorithm = algorithm;
        self
    }
}

#[async_trait::async_trait]
impl KeySource for EnvKeySource {
    #[instrument(skip(self), fields(prefix = %self.prefix, keys = self.key_ids.len()), err(Debug))]
    async fn load_keys(&self) -> KeyResult<Vec<KeyConfig>> {
        use base64::{Engine, engine::general_purpose::STANDARD};

        let mut configs = Vec::new();

        for id in &self.key_ids {
            let env_var = format!("{}_{}", self.prefix, id.to_uppercase());
            let key_b64 = self
                .vars
                .get(&env_var)
                .ok_or_else(|| KeyError::KeyNotFound(format!("env var {} not set", env_var)))?;

            let material = STANDARD.decode(&key_b64).map_err(|e| {
                KeyError::InvalidConfig(format!("invalid base64 in {}: {}", env_var, e))
            })?;

            if material.len() != 32 {
                return Err(KeyError::InvalidConfig(format!(
                    "key in {} must be 32 bytes, got {}",
                    env_var,
                    material.len()
                )));
            }

            // A present but unparseable version is a misconfiguration, not a default.
            let version_var = format!("{}_{}_VERSION", self.prefix, id.to_uppercase());
            let version = match self.vars.get(&version_var) {
                Some(raw) => match raw.parse::<u32>() {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(
                            version_var = %version_var,
                            value = %raw,
                            error = %e,
                            "key version override is not a valid integer; defaulting to version 1"
                        );
                        1
                    }
                },
                None => 1,
            };

            configs.push(KeyConfig {
                id: id.clone(),
                material,
                version,
                algorithm: self.algorithm,
                purpose: String::new(),
                fingerprint_declared: false,
            });
        }

        Ok(configs)
    }
}
