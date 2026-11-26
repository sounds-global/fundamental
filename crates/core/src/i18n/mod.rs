//! Translation primitives shared across the framework.

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
#[cfg(feature = "std")]
use std::borrow::Cow;

mod args;
mod checked;
mod languages;
mod locale;
mod localized;
mod translation_check;
mod translation_gap;
mod translator;
mod translator_source;

pub use args::{Arg, ArgValue, interpolate};
pub use checked::Checked;
pub use languages::Languages;
pub use locale::{Locale, LocaleParseError};
pub use localized::{Localized, source_fingerprint};
pub use translation_check::{TranslationCheck, TranslationRow};
pub use translation_gap::{NotReadyToPublish, TranslationGap};
pub use translator::{NoopTranslator, Translator};
#[cfg(feature = "std")]
pub use translator_source::TranslatorSource;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Builds a locale fallback chain: `requested → language-only → extra → source`, de-duplicated.
pub fn negotiate_chain(requested: &Locale, extra: &[Locale], source: &Locale) -> Vec<Locale> {
    let mut chain: Vec<Locale> = Vec::with_capacity(extra.len() + 3);
    let push = |loc: Locale, chain: &mut Vec<Locale>| {
        if !chain.contains(&loc) {
            chain.push(loc);
        }
    };
    push(requested.clone(), &mut chain);
    if !requested.is_language_only() {
        push(requested.language_fallback(), &mut chain);
    }
    for f in extra {
        push(f.clone(), &mut chain);
    }
    push(source.clone(), &mut chain);
    chain
}

/// Resolves `key` for the current locale, falling back to the source template.
///
/// The `fallback` closure runs only when no installed translator resolves the key.
///
/// ```
/// use std::borrow::Cow;
/// use fundamental_core::i18n::translate;
///
/// // With no translator installed, the fallback closure result is returned.
/// let msg = translate("error.demo", &[], || Cow::Borrowed("hello"));
/// assert_eq!(msg, "hello");
/// ```
#[cfg_attr(
    not(feature = "std"),
    expect(
        unused_variables,
        reason = "without `std` there is no translator to hand them to"
    )
)]
pub fn translate<'a>(
    key: &str,
    args: &[Arg<'_>],
    fallback: impl FnOnce() -> Cow<'a, str>,
) -> Cow<'a, str> {
    #[cfg(feature = "std")]
    {
        if let Some(translator) = ambient::current()
            && let Some(rendered) = translator.lookup(key, args)
        {
            return Cow::Owned(rendered);
        }
    }
    fallback()
}

#[cfg(feature = "std")]
pub use ambient::{current, set_translator, set_translator_source, translator_source, with};

#[cfg(feature = "std")]
mod ambient {
    use std::cell::RefCell;
    use std::sync::{Arc, RwLock};

    use super::{Translator, TranslatorSource};

    /// Process-wide installed translator.
    static GLOBAL: RwLock<Option<Arc<dyn Translator>>> = RwLock::new(None);

    thread_local! {
        /// Scoped overrides pushed by [`with`]; the top of the stack wins.
        static SCOPED: RefCell<Vec<Arc<dyn Translator>>> = const { RefCell::new(Vec::new()) };
    }

    /// Process-wide translator source, one translator per locale; independent of `GLOBAL`.
    static SOURCE: RwLock<Option<Arc<dyn TranslatorSource>>> = RwLock::new(None);

    /// Installs `translator` as the process-wide default.
    pub fn set_translator(translator: Arc<dyn Translator>) {
        // Recover a poisoned guard so it cannot wedge translation for the whole run.
        let mut slot = GLOBAL.write().unwrap_or_else(|e| e.into_inner());
        *slot = Some(translator);
    }

    /// Installs `source` as the process-wide way to obtain a translator for a locale.
    ///
    /// It decides no current locale: a caller scopes the source's translator with [`with`].
    pub fn set_translator_source(source: Arc<dyn TranslatorSource>) {
        let mut slot = SOURCE.write().unwrap_or_else(|e| e.into_inner());
        *slot = Some(source);
    }

    /// Returns the installed translator source, if any.
    pub fn translator_source() -> Option<Arc<dyn TranslatorSource>> {
        SOURCE
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
    }

    /// Returns the active translator: the innermost [`with`] scope, else the process-wide default.
    pub fn current() -> Option<Arc<dyn Translator>> {
        if let Some(scoped) = SCOPED.with(|s| s.borrow().last().cloned()) {
            return Some(scoped);
        }
        GLOBAL
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
    }

    /// Runs `f` with `translator` active on the current thread.
    ///
    /// The override is popped when `f` returns or unwinds.
    pub fn with<R>(translator: Arc<dyn Translator>, f: impl FnOnce() -> R) -> R {
        struct PopGuard;
        impl Drop for PopGuard {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drop(&mut self) {
                SCOPED.with(|s| {
                    s.borrow_mut().pop();
                });
            }
        }

        SCOPED.with(|s| s.borrow_mut().push(translator));
        let _guard = PopGuard;
        f()
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::sync::Arc;

    struct FakeTranslator;
    impl Translator for FakeTranslator {
        fn lookup(&self, key: &str, args: &[Arg<'_>]) -> Option<String> {
            if key == "error.user_001" {
                let id = args.iter().find(|(n, _)| *n == "id").map(|(_, v)| v);
                Some(match id {
                    Some(v) => format!("Usuario {v} no encontrado"),
                    None => "Usuario no encontrado".to_string(),
                })
            } else {
                None
            }
        }
    }

    #[test]
    fn falls_back_without_translator() {
        let msg = translate("error.user_001", &[], || Cow::Borrowed("User not found"));
        assert_eq!(msg, "User not found");
    }

    #[test]
    fn uses_scoped_translator_and_pops() {
        let id = 7;
        let args = [("id", ArgValue::display(&id))];
        let rendered = with(Arc::new(FakeTranslator), || {
            translate("error.user_001", &args, || {
                Cow::Owned(format!("User {id} not found"))
            })
            .into_owned()
        });
        assert_eq!(rendered, "Usuario 7 no encontrado");

        let after = translate("error.user_001", &args, || {
            Cow::Owned(format!("User {id} not found"))
        });
        assert_eq!(after, "User 7 not found");
    }

    #[test]
    fn missing_key_falls_through_to_source() {
        let msg = with(Arc::new(FakeTranslator), || {
            translate("error.other", &[], || Cow::Borrowed("Other")).into_owned()
        });
        assert_eq!(msg, "Other");
    }

    struct FakeSource;
    impl TranslatorSource for FakeSource {
        fn translator(&self, _locale: &Locale) -> Arc<dyn Translator> {
            Arc::new(FakeTranslator)
        }
    }

    /// A source is not a language: installing one must not decide what `translate` renders.
    #[test]
    fn installing_a_source_does_not_make_a_locale_current() {
        set_translator_source(Arc::new(FakeSource));
        assert!(translator_source().is_some());
        assert!(
            current().is_none(),
            "a source has no opinion about which locale is current"
        );
        assert_eq!(
            translate("error.user_001", &[], || Cow::Borrowed("User not found")),
            "User not found"
        );
    }

    #[test]
    fn a_scope_from_the_source_renders_and_then_pops() {
        set_translator_source(Arc::new(FakeSource));
        let locale: Locale = "es".parse().expect("a language tag");
        let source = translator_source().expect("installed just above");

        let rendered = with(source.translator(&locale), || {
            translate("error.user_001", &[], || Cow::Borrowed("User not found")).into_owned()
        });
        assert_eq!(rendered, "Usuario no encontrado");
        assert_eq!(
            translate("error.user_001", &[], || Cow::Borrowed("User not found")),
            "User not found",
            "the scope is the request's, not the process's"
        );
    }
}
