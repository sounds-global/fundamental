//! [`CatalogTranslator`] — a [`Catalog`]-backed [`Translator`] implementation.

use std::sync::Arc;

use fundamental_core::i18n::{
    Arg, ArgValue, Locale, Translator, TranslatorSource, negotiate_chain,
};
use fundamental_messages::{Catalogs, Value};
use tracing::debug;

use crate::Catalog;

/// Resolves keys against a [`Catalog`] for a negotiated locale fallback chain.
#[derive(Clone, Debug)]
pub struct CatalogTranslator {
    catalog: Catalog,
    /// Locales to try in order, ending at the catalog's source locale.
    chain: Vec<Locale>,
    primary_tag: String,
}

impl Catalog {
    /// Builds a translator that resolves `requested` for this catalog.
    ///
    /// The fallback chain is `requested → language-only → source locale`.
    pub fn translator(&self, requested: &Locale) -> CatalogTranslator {
        let chain = negotiate_chain(requested, &[], self.source_locale());
        debug!(requested = %requested, chain = ?chain, "negotiated locale fallback chain");
        CatalogTranslator {
            catalog: self.clone(),
            primary_tag: requested.to_string(),
            chain,
        }
    }
}

/// A catalog is a [`TranslatorSource`]: each request scopes its own language from it.
impl TranslatorSource for Catalog {
    fn translator(&self, locale: &Locale) -> Arc<dyn Translator> {
        Arc::new(Catalog::translator(self, locale))
    }
}

impl CatalogTranslator {
    /// The negotiated fallback chain, most-specific first.
    pub fn chain(&self) -> &[Locale] {
        &self.chain
    }

    /// `key` said by the first crate whose messages have it, in the reader's language.
    fn say_from_messages(&self, key: &str, args: &[Arg<'_>]) -> Option<String> {
        self.say_from(&self.catalog.inner.messages, key, args)
    }

    /// `key` said by the first of `catalogs` that has it, in the reader's language.
    fn say_from(
        &self,
        catalogs: &[&'static Catalogs],
        key: &str,
        args: &[Arg<'_>],
    ) -> Option<String> {
        let values: Vec<(&str, Value)> = args
            .iter()
            .map(|(name, value)| (*name, to_value(value)))
            .collect();
        catalogs
            .iter()
            .find(|catalogs| catalogs.carries(key))
            .and_then(|catalogs| catalogs.say(key, &self.primary_tag, &values))
    }
}

fn to_value(value: &ArgValue<'_>) -> Value {
    match value {
        ArgValue::Int(number) => Value::Integer(*number),
        ArgValue::Float(number) => Value::Number(*number),
        ArgValue::Str(_) | ArgValue::Owned(_) | ArgValue::Display(_) => Value::text(value),
    }
}

impl Translator for CatalogTranslator {
    fn lookup(&self, key: &str, args: &[Arg<'_>]) -> Option<String> {
        let said = self
            .own_words(key, args)
            .or_else(|| self.say_from_messages(key, args));
        if said.is_none() {
            // Every locale in the chain missed: the call site renders its source template.
            debug!(
                key,
                "translation key missing in all chain locales; using source fallback"
            );
        }
        said
    }

    fn own_words(&self, id: &str, args: &[Arg<'_>]) -> Option<String> {
        self.say_from(&self.catalog.inner.own, id, args)
    }

    fn carries(&self, key: &str) -> bool {
        self.catalog
            .inner
            .messages
            .iter()
            .chain(&self.catalog.inner.own)
            .any(|catalogs| catalogs.carries(key))
    }

    fn locale_tag(&self) -> &str {
        &self.primary_tag
    }

    fn locale_chain(&self) -> Vec<Locale> {
        self.chain.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn missing_key_logs_a_fallback() {
        let en: Locale = "en".parse().expect("valid locale");
        let translator = Catalog::builder(en.clone()).build().translator(&en);

        assert!(translator.lookup("absent_key", &[]).is_none());
        assert!(
            logs_contain("translation key missing"),
            "a missing key must be logged so absent translations are diagnosable"
        );
    }
}
