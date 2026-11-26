//! Localization configuration ([`TranslationSettings`]).

use serde::{Deserialize, Serialize};

/// Localization configuration: the languages an app serves and their fallback chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslationSettings {
    /// Whether localization is active; when `false` every message renders in its source locale.
    pub enabled: bool,

    /// The BCP-47 language the app's messages are authored in, and the bottom of every fallback chain.
    pub source_locale: String,

    /// The locale served to a user whose preference is unknown, when it differs from the source locale.
    #[serde(default)]
    pub default_locale: Option<String>,

    /// Every locale the app serves, including the source and default locales.
    pub supported_locales: Vec<String>,

    /// Extra fallback locales to try, in order, before the source locale.
    pub fallback_chain: Vec<String>,
}

impl Default for TranslationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            source_locale: "en".to_string(),
            default_locale: None,
            supported_locales: vec!["en".to_string()],
            fallback_chain: Vec::new(),
        }
    }
}

impl TranslationSettings {
    /// The locale served to users with no preference: `default_locale`, else `source_locale`.
    #[must_use]
    pub fn default_locale(&self) -> &str {
        self.default_locale
            .as_deref()
            .unwrap_or(&self.source_locale)
    }

    /// Configuration errors when enabled (empty when valid).
    pub(crate) fn validation_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !self.enabled {
            return errors;
        }
        if self.supported_locales.is_empty() {
            errors
                .push("i18n.supported_locales must not be empty when i18n is enabled".to_string());
        }
        if !self.supported_locales.contains(&self.source_locale) {
            errors.push(format!(
                "i18n.source_locale '{}' must be listed in supported_locales",
                self.source_locale
            ));
        }
        let default_locale = self.default_locale();
        if !self.supported_locales.iter().any(|l| l == default_locale) {
            errors.push(format!(
                "i18n.default_locale '{default_locale}' must be listed in supported_locales"
            ));
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled_and_valid() {
        let s = TranslationSettings::default();
        assert!(!s.enabled);
        assert_eq!(s.source_locale, "en");
        assert!(s.validation_errors().is_empty());
    }

    #[test]
    fn enabled_requires_source_and_default_in_supported() {
        let mut s = TranslationSettings {
            enabled: true,
            source_locale: "es".to_string(),
            default_locale: Some("fr".to_string()),
            supported_locales: vec!["en".to_string()],
            ..Default::default()
        };
        let errors = s.validation_errors();
        assert_eq!(errors.len(), 2, "{errors:?}");

        s.supported_locales = vec!["en".to_string(), "es".to_string(), "fr".to_string()];
        assert!(s.validation_errors().is_empty());
    }

    #[test]
    fn deserializes_from_toml() {
        let toml = r#"
            enabled = true
            source_locale = "en"
            default_locale = "en"
            supported_locales = ["en", "es", "fr"]
            fallback_chain = ["es"]
        "#;
        let s: TranslationSettings = toml::from_str(toml).unwrap();
        assert!(s.enabled);
        assert_eq!(s.supported_locales, ["en", "es", "fr"]);
        assert_eq!(s.fallback_chain, ["es"]);
    }
}
