use std::sync::Arc;

use fundamental_core::i18n::{self, Locale, Translator};
use tracing::debug;

/// Runs `render` inside `locale`'s translator; synchronous because the scope is thread-local.
pub(crate) fn in_language<T>(locale: Option<&Locale>, render: impl FnOnce() -> T) -> T {
    match translator_for(locale) {
        Some(translator) => i18n::with(translator, render),
        None => render(),
    }
}

/// The translator a request in `locale` is answered in, if any.
pub(crate) fn translator_for(locale: Option<&Locale>) -> Option<Arc<dyn Translator>> {
    let locale = locale?;
    let Some(source) = i18n::translator_source() else {
        debug!(
            locale = %locale,
            "rendering in the source strings: a locale was negotiated but no translator source is installed"
        );
        return None;
    };
    Some(source.translator(locale))
}
