//! [`TranslatorSource`] — a translator per locale, rather than one for the process.

#[cfg(feature = "std")]
use std::sync::Arc;

#[cfg(feature = "std")]
use super::{Locale, Translator};

/// Supplies the [`Translator`] for a locale, so one process can answer two callers in two languages.
#[cfg(feature = "std")]
pub trait TranslatorSource: Send + Sync + 'static {
    /// The translator that renders `locale`.
    ///
    /// It cannot fail: a page in the wrong language beats a page that did not render.
    fn translator(&self, locale: &Locale) -> Arc<dyn Translator>;
}
