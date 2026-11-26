use tracing::{debug, info, instrument, warn};

use crate::{KeyConfig, KeyError, KeyResult, KeySource};

/// A key source that tries multiple sources in order.
///
/// The first source with a non-empty key set wins; when none does, the last error is returned.
///
/// # Examples
///
/// ```ignore
/// use fundamental_encryption::{
///     CompositeKeySource, EnvKeySource, KeyManagerBuilder,
/// };
///
/// // This works in both environments:
/// // - Production: Uses KMS-encrypted keys
/// // - Development: Falls back to FUNDAMENTAL_KEY_* environment variables
/// let source = CompositeKeySource::new()
///     .with_source(Box::new(envelope_source))  // Try KMS first
///     .with_source(Box::new(EnvKeySource::new(vec!["default".into(), "pii".into()])));
///
/// let key_manager = KeyManagerBuilder::from_source(&source).await?;
/// ```
pub struct CompositeKeySource {
    sources: Vec<Box<dyn KeySource>>,
}

impl CompositeKeySource {
    /// Creates a new composite key source.
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    /// Adds a key source to try.
    pub fn with_source(mut self, source: Box<dyn KeySource>) -> Self {
        self.sources.push(source);
        self
    }
}

impl Default for CompositeKeySource {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CompositeKeySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeKeySource")
            .field("sources", &self.sources.len())
            .finish()
    }
}

#[async_trait::async_trait]
impl KeySource for CompositeKeySource {
    #[instrument(skip(self), fields(sources = self.sources.len()), err(Debug))]
    async fn load_keys(&self) -> KeyResult<Vec<KeyConfig>> {
        let mut last_error = None;

        for (source_index, source) in self.sources.iter().enumerate() {
            match source.load_keys().await {
                Ok(keys) if !keys.is_empty() => {
                    // A demotion from a higher-priority source is a security-posture change.
                    info!(
                        source_index,
                        keys = keys.len(),
                        "key source provided keys; using this source"
                    );
                    return Ok(keys);
                }
                Ok(_) => {
                    debug!(source_index, "key source returned no keys; trying next");
                    continue;
                }
                Err(e) => {
                    warn!(source_index, error = %e, "key source failed; trying next");
                    last_error = Some(e);
                    continue;
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| KeyError::InvalidConfig("no key sources configured".to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    enum Scripted {
        Keys(Vec<KeyConfig>),
        Empty,
        Fail(&'static str),
    }

    #[async_trait::async_trait]
    impl KeySource for Scripted {
        async fn load_keys(&self) -> KeyResult<Vec<KeyConfig>> {
            match self {
                Scripted::Keys(k) => Ok(k.clone()),
                Scripted::Empty => Ok(Vec::new()),
                Scripted::Fail(msg) => Err(KeyError::Kms((*msg).to_string())),
            }
        }
    }

    fn one_key() -> Vec<KeyConfig> {
        vec![KeyConfig::new("default", vec![0u8; 32])]
    }

    #[traced_test]
    #[tokio::test]
    async fn first_failure_is_logged_then_fallback_succeeds() {
        let source = CompositeKeySource::new()
            .with_source(Box::new(Scripted::Fail("kms unreachable")))
            .with_source(Box::new(Scripted::Keys(one_key())));

        let keys = source.load_keys().await.expect("fallback yields keys");
        assert_eq!(keys.len(), 1);
        assert!(logs_contain("key source failed; trying next"));
        assert!(logs_contain("kms unreachable"));
        assert!(logs_contain("key source provided keys; using this source"));
    }

    #[traced_test]
    #[tokio::test]
    async fn empty_source_is_logged_and_skipped() {
        let source = CompositeKeySource::new()
            .with_source(Box::new(Scripted::Empty))
            .with_source(Box::new(Scripted::Keys(one_key())));

        let keys = source.load_keys().await.expect("fallback yields keys");
        assert_eq!(keys.len(), 1);
        assert!(logs_contain("key source returned no keys; trying next"));
    }

    #[traced_test]
    #[tokio::test]
    async fn all_sources_failing_propagates_last_error() {
        let source = CompositeKeySource::new()
            .with_source(Box::new(Scripted::Fail("first")))
            .with_source(Box::new(Scripted::Fail("second")));

        let err = source.load_keys().await.expect_err("all sources failed");
        assert!(matches!(err, KeyError::Kms(ref m) if m == "second"));
        assert!(logs_contain("key source failed; trying next"));
    }

    #[tokio::test]
    async fn no_sources_configured_is_invalid_config() {
        let source = CompositeKeySource::new();
        let err = source.load_keys().await.expect_err("no sources");
        assert!(matches!(err, KeyError::InvalidConfig(_)));
    }
}
