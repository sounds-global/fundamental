use fundamental_core::secrets::SecretString;
use tracing::instrument;

use super::matches_declared_fingerprint::matches_declared_fingerprint;
use crate::fns::read_labelled_material;
use crate::{EncryptionAlgorithm, KeyConfig, KeyError, KeyResult, KeySource};

/// One key's material as configured, before decoding.
#[derive(Debug, Clone)]
pub struct ConfiguredKey {
    /// The key identifier.
    pub id: String,
    /// The purpose the material must be labelled for.
    pub purpose: String,
    /// The key material, `<purpose>:<version>:<64 hex>`.
    pub material: SecretString,
    /// The key version.
    pub version: u32,
    /// The AEAD algorithm this key is used with.
    pub algorithm: EncryptionAlgorithm,
    /// The fingerprint the material must have, as 64 hex characters, when one is declared.
    pub fingerprint: Option<String>,
}

/// A key source that loads keys from values supplied by configuration.
///
/// Unlike [`EnvKeySource`](crate::EnvKeySource), the material never enters the process
/// environment (readable from `/proc/<pid>/environ`); a `${file:…}` reference reads a mounted
/// secret directly. Material must be labelled `<purpose>:<version>:<64 hex>` for its slot.
///
/// ```no_run
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// use fundamental_config::AppSettings;
/// use fundamental_encryption::KeyManager;
///
/// let app = AppSettings::load("layer1")?;
/// // `backend = "config"` selects this source.
/// let keys = KeyManager::from_settings(&app.config().encryption, None).await?;
/// # let _ = keys;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ConfiguredKeySource {
    keys: Vec<ConfiguredKey>,
}

impl ConfiguredKeySource {
    /// Creates a source over already-described keys.
    #[must_use]
    pub fn new(keys: Vec<ConfiguredKey>) -> Self {
        Self { keys }
    }
}

#[async_trait::async_trait]
impl KeySource for ConfiguredKeySource {
    #[instrument(skip(self), fields(keys = self.keys.len()), err(Debug))]
    async fn load_keys(&self) -> KeyResult<Vec<KeyConfig>> {
        let mut configs = Vec::with_capacity(self.keys.len());
        let mut problems = Vec::new();

        for key in &self.keys {
            // Every failure names the id and never the material.
            if key.purpose.trim().is_empty() {
                problems.push(format!("key '{}' names no purpose", key.id));
                continue;
            }
            if key.version == 0 {
                problems.push(format!(
                    "key '{}': version 0 is reserved; versions start at 1",
                    key.id
                ));
                continue;
            }
            match read_labelled_material(
                key.material.expose(),
                &key.purpose,
                &key.version.to_string(),
            ) {
                Ok(material) => {
                    let declared = match &key.fingerprint {
                        None => false,
                        Some(expected) => {
                            if !matches_declared_fingerprint(expected, &material[..]) {
                                problems.push(format!(
                                    "key '{}' version {}: the material does not match the fingerprint declared for it",
                                    key.id, key.version
                                ));
                                continue;
                            }
                            true
                        }
                    };
                    configs.push(KeyConfig {
                        id: key.id.clone(),
                        material: material.to_vec(),
                        version: key.version,
                        algorithm: key.algorithm,
                        purpose: key.purpose.clone(),
                        fingerprint_declared: declared,
                    });
                }
                Err(problem) => problems.push(format!(
                    "key '{}' version {}: {problem}",
                    key.id, key.version
                )),
            }
        }

        if !problems.is_empty() {
            return Err(KeyError::InvalidConfig(problems.join("; ")));
        }
        Ok(configs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::secrets::Secret;

    fn configured(id: &str, purpose: &str, material: String) -> ConfiguredKey {
        ConfiguredKey {
            id: id.to_string(),
            purpose: purpose.to_string(),
            material: Secret::new(material),
            version: 1,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            fingerprint: None,
        }
    }

    fn fingerprint_of(byte: u8) -> String {
        crate::material_fingerprint(&[byte; 32])
            .iter()
            .map(|part| format!("{part:02x}"))
            .collect()
    }

    #[tokio::test]
    async fn a_key_matching_its_declared_fingerprint_loads_as_declared() {
        let mut key = configured(
            "pii",
            "app.at-rest.v1",
            format!("app.at-rest.v1:1:{}", "07".repeat(32)),
        );
        key.fingerprint = Some(fingerprint_of(7).to_uppercase());
        let keys = ConfiguredKeySource::new(vec![key])
            .load_keys()
            .await
            .expect("the declared key loads");
        assert!(keys[0].fingerprint_declared);
    }

    /// Another deployment's key file refuses the boot.
    #[tokio::test]
    async fn a_key_that_does_not_match_its_declared_fingerprint_fails_the_load() {
        let hex = "07".repeat(32);
        let mut key = configured("pii", "app.at-rest.v1", format!("app.at-rest.v1:1:{hex}"));
        key.fingerprint = Some(fingerprint_of(8));
        let rendered = ConfiguredKeySource::new(vec![key])
            .load_keys()
            .await
            .expect_err("material that is not the declared key")
            .to_string();
        assert!(
            rendered.contains("'pii' version 1") && rendered.contains("declared"),
            "{rendered}"
        );
        assert!(!rendered.contains(&hex), "{rendered}");
    }

    #[tokio::test]
    async fn a_key_with_no_declared_fingerprint_loads_undeclared() {
        let keys = ConfiguredKeySource::new(vec![configured(
            "pii",
            "app.at-rest.v1",
            format!("app.at-rest.v1:1:{}", "07".repeat(32)),
        )])
        .load_keys()
        .await
        .expect("the key loads");
        assert!(!keys[0].fingerprint_declared);
    }

    #[tokio::test]
    async fn a_labelled_key_loads_without_touching_the_environment() {
        let source = ConfiguredKeySource::new(vec![configured(
            "pii",
            "app.at-rest.v1",
            format!("app.at-rest.v1:1:{}", "07".repeat(32)),
        )]);

        let keys = source.load_keys().await.expect("the key decodes");

        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].id, "pii");
        assert_eq!(keys[0].material, vec![7u8; 32]);
    }

    /// One service's key file in another's slot names the key and both purposes, never the material.
    #[tokio::test]
    async fn a_key_labelled_for_another_purpose_fails_the_load() {
        let hex = "07".repeat(32);
        let source = ConfiguredKeySource::new(vec![configured(
            "pii",
            "app.layer-four.v1",
            format!("app.layer-one.v1:1:{hex}"),
        )]);

        let error = source.load_keys().await.expect_err("another slot's key");

        let rendered = error.to_string();
        assert!(rendered.contains("pii"), "{rendered}");
        assert!(rendered.contains("app.layer-one.v1"), "{rendered}");
        assert!(!rendered.contains(&hex), "{rendered}");
    }

    /// Unlabelled base64 material refuses.
    #[tokio::test]
    async fn unlabelled_material_fails_the_load() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let source = ConfiguredKeySource::new(vec![configured(
            "pii",
            "app.at-rest.v1",
            STANDARD.encode([7u8; 32]),
        )]);

        let error = source.load_keys().await.expect_err("an unlabelled key");

        assert!(error.to_string().contains("expected `<purpose>"), "{error}");
    }

    /// An empty `${file:…:-}` expansion refuses.
    #[tokio::test]
    async fn empty_material_fails_the_load() {
        let source =
            ConfiguredKeySource::new(vec![configured("pii", "app.at-rest.v1", String::new())]);

        source.load_keys().await.expect_err("a missing key file");
    }

    #[tokio::test]
    async fn every_problem_is_reported_at_once() {
        let mut zero = configured(
            "mail",
            "app.mail.v1",
            format!("app.mail.v1:0:{}", "08".repeat(32)),
        );
        zero.version = 0;
        let source = ConfiguredKeySource::new(vec![
            configured("pii", "", format!("app.at-rest.v1:1:{}", "07".repeat(32))),
            zero,
            configured(
                "audit",
                "app.audit.v1",
                format!("app.audit.v1:1:{}", "00".repeat(32)),
            ),
        ]);

        let rendered = source
            .load_keys()
            .await
            .expect_err("three bad keys")
            .to_string();

        assert!(rendered.contains("'pii' names no purpose"), "{rendered}");
        assert!(rendered.contains("'mail': version 0"), "{rendered}");
        assert!(
            rendered.contains("'audit' version 1: an all-zero"),
            "{rendered}"
        );
    }
}
