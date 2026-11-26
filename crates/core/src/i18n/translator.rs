//! The [`Translator`] contract and the inert [`NoopTranslator`] default.

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec, vec::Vec};

use super::{Arg, Locale};

/// Looks up and renders localized messages.
pub trait Translator: Send + Sync + 'static {
    /// Resolves `key` for the current locale and interpolates `args`.
    ///
    /// `None` when the key is absent in every locale of the fallback chain.
    fn lookup(&self, key: &str, args: &[Arg<'_>]) -> Option<String>;

    /// Whether `key` has a message in the active fallback chain — the question a boot-time
    ///
    /// [`lookup`](Self::lookup) cannot answer it for a message that takes arguments.
    fn carries(&self, key: &str) -> bool {
        self.lookup(key, &[]).is_some()
    }

    /// The app's own words for message `id` of a crate's messages, said with `args`; `None` when it has none.
    fn own_words(&self, id: &str, args: &[Arg<'_>]) -> Option<String> {
        let _ = (id, args);
        None
    }

    /// The active locale, as a BCP-47 tag (e.g. `"es-MX"`).
    ///
    /// Defaults to `"und"` (undetermined).
    fn locale_tag(&self) -> &str {
        "und"
    }

    /// The negotiated locale fallback chain, most-specific first.
    ///
    /// The default derives a best-effort chain from [`locale_tag`](Self::locale_tag) (`es-MX` → `es`).
    fn locale_chain(&self) -> Vec<Locale> {
        match self.locale_tag().parse::<Locale>() {
            Ok(loc) if loc.is_language_only() => vec![loc],
            Ok(loc) => {
                let lang = loc.language_fallback();
                vec![loc, lang]
            }
            Err(_) => Vec::new(),
        }
    }
}

/// A [`Translator`] that resolves nothing — every lookup misses.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopTranslator;

impl Translator for NoopTranslator {
    fn lookup(&self, _key: &str, _args: &[Arg<'_>]) -> Option<String> {
        None
    }
}
