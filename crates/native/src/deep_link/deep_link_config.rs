/// Activates native deep-link delivery and names the custom URL schemes the app answers.
///
/// `schemes` are registered at runtime on desktop Linux and Windows; the authoritative
/// declaration, universal-link domains included, lives in the build-time platform config.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeepLinkConfig {
    schemes: Vec<String>,
}

impl DeepLinkConfig {
    /// Activates deep-link delivery, claiming the custom URL `schemes` (without `://`).
    #[must_use]
    pub fn new(schemes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            schemes: schemes.into_iter().map(Into::into).collect(),
        }
    }

    /// The custom URL schemes the app claims (for desktop runtime registration).
    #[must_use]
    pub fn schemes(&self) -> &[String] {
        &self.schemes
    }
}

#[cfg(test)]
mod tests {
    use super::DeepLinkConfig;

    #[test]
    fn new_collects_the_named_schemes() {
        let config = DeepLinkConfig::new(["openid4vp", "acme"]);
        assert_eq!(config.schemes(), ["openid4vp", "acme"]);
    }

    #[test]
    fn default_claims_no_schemes() {
        assert!(DeepLinkConfig::default().schemes().is_empty());
    }
}
