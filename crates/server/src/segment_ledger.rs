use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use fundamental_core::address::LanguagePaths;
use fundamental_core::i18n::Locale;

use crate::route_pattern::RoutePattern;
use crate::{AuthorizedObjects, ResolvedSegment, SegmentResolution};

/// What a request's declared segments were asked as and resolved to, and which records authorization affirmed.
///
/// Nothing about a resolved record leaves the server until every declared segment is affirmed.
#[derive(Clone, Default)]
pub struct SegmentLedger {
    state: Arc<Mutex<LedgerState>>,
}

#[derive(Default)]
struct LedgerState {
    requested: Vec<(String, String)>,
    outcomes: Vec<(String, Outcome)>,
    namespaces: Vec<(String, String)>,
    affirmed: Vec<String>,
}

enum Outcome {
    Canonical(ResolvedSegment),
    Moved(ResolvedSegment),
    Unknown,
}

impl SegmentLedger {
    /// Whether this request had declared segments at all.
    #[must_use]
    pub fn is_declared(&self) -> bool {
        !self.lock().outcomes.is_empty()
    }

    /// The record `param` resolved to, if it named one.
    #[must_use]
    pub fn segment(&self, param: &str) -> Option<ResolvedSegment> {
        self.lock()
            .outcomes
            .iter()
            .find(|(name, _)| name == param)
            .and_then(|(_, outcome)| match outcome {
                Outcome::Canonical(segment) | Outcome::Moved(segment) => Some(segment.clone()),
                Outcome::Unknown => None,
            })
    }

    /// `param` as the request spelled it — what a return path rebuilds from.
    #[must_use]
    pub fn requested(&self, param: &str) -> Option<String> {
        self.lock()
            .requested
            .iter()
            .find(|(name, _)| name == param)
            .map(|(_, raw)| raw.clone())
    }

    /// Records that the reader may see the record `param` names, for a loader that checked access itself.
    pub fn affirm(&self, param: &str) {
        let mut state = self.lock();
        if !state.affirmed.iter().any(|known| known == param) {
            state.affirmed.push(param.to_owned());
        }
    }

    /// Whether every declared segment named a record and authorization affirmed each one.
    #[must_use]
    pub fn disclosable(&self, authorized: &AuthorizedObjects) -> bool {
        let state = self.lock();
        !state.outcomes.is_empty()
            && state.outcomes.iter().all(|(param, outcome)| match outcome {
                Outcome::Canonical(segment) | Outcome::Moved(segment) => {
                    state.affirmed.iter().any(|known| known == param)
                        || state
                            .namespaces
                            .iter()
                            .find(|(name, _)| name == param)
                            .is_some_and(|(_, namespace)| {
                                authorized.contains(namespace, segment.id())
                            })
                }
                Outcome::Unknown => false,
            })
    }

    /// The page's path in every language with a spelling of its own, or `None`.
    #[must_use]
    pub fn language_paths(&self, template: &str) -> Option<LanguagePaths> {
        let pattern = RoutePattern::parse(template);
        let mut paths = LanguagePaths::uniform(self.fill_default(&pattern)?);
        for locale in self.spelled_languages() {
            paths = paths.with(locale.clone(), self.fill(&pattern, Some(&locale))?);
        }
        Some(paths)
    }

    /// `pattern` filled with each segment's default spelling.
    fn fill_default(&self, pattern: &RoutePattern) -> Option<String> {
        let state = self.lock();
        pattern.fill(
            |param| match state.outcomes.iter().find(|(name, _)| name == param) {
                Some((_, Outcome::Canonical(segment) | Outcome::Moved(segment))) => {
                    Some(segment.default_spelling())
                }
                Some((_, Outcome::Unknown)) => None,
                None => state
                    .requested
                    .iter()
                    .find(|(name, _)| name == param)
                    .map(|(_, raw)| raw.as_str()),
            },
        )
    }

    /// Whether any segment was asked for in a spelling other than its canonical one.
    pub(crate) fn moved(&self) -> bool {
        self.lock()
            .outcomes
            .iter()
            .any(|(_, outcome)| matches!(outcome, Outcome::Moved(_)))
    }

    /// `pattern` filled with each segment as `locale` spells it, every other capture as requested.
    pub(crate) fn fill(&self, pattern: &RoutePattern, locale: Option<&Locale>) -> Option<String> {
        let state = self.lock();
        pattern.fill(|param| {
            let resolved = state.outcomes.iter().find(|(name, _)| name == param);
            match resolved {
                Some((_, Outcome::Canonical(segment) | Outcome::Moved(segment))) => {
                    Some(match locale {
                        Some(locale) => segment.spelling_in(locale),
                        None => segment.canonical(),
                    })
                }
                Some((_, Outcome::Unknown)) => None,
                None => state
                    .requested
                    .iter()
                    .find(|(name, _)| name == param)
                    .map(|(_, raw)| raw.as_str()),
            }
        })
    }

    pub(crate) fn record_requested(&self, param: &str, raw: &str) {
        self.lock()
            .requested
            .push((param.to_owned(), raw.to_owned()));
    }

    pub(crate) fn record(
        &self,
        param: &str,
        namespace: Option<&str>,
        resolution: SegmentResolution,
    ) {
        let outcome = match resolution {
            SegmentResolution::Canonical(segment) => Outcome::Canonical(segment),
            SegmentResolution::Moved(segment) => Outcome::Moved(segment),
            SegmentResolution::Unknown => Outcome::Unknown,
        };
        let mut state = self.lock();
        state.outcomes.push((param.to_owned(), outcome));
        if let Some(namespace) = namespace {
            state
                .namespaces
                .push((param.to_owned(), namespace.to_owned()));
        }
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn spelled_languages(&self) -> Vec<Locale> {
        let state = self.lock();
        let mut languages: Vec<Locale> = Vec::new();
        for (_, outcome) in &state.outcomes {
            if let Outcome::Canonical(segment) | Outcome::Moved(segment) = outcome {
                for locale in segment.spelled_languages() {
                    if !languages.contains(locale) {
                        languages.push(locale.clone());
                    }
                }
            }
        }
        languages
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LedgerState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for SegmentLedger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SegmentLedger")
    }
}

#[cfg(test)]
mod tests {
    use super::SegmentLedger;
    use crate::{AuthorizedObjects, ResolvedSegment, SegmentResolution};

    fn segment(id: &str, canonical: &str) -> ResolvedSegment {
        ResolvedSegment::new(id, canonical).expect("a valid segment")
    }

    #[test]
    fn nothing_is_disclosable_until_every_segment_is_affirmed() {
        let ledger = SegmentLedger::default();
        let authorized = AuthorizedObjects::default();
        assert!(
            !ledger.disclosable(&authorized),
            "no segments, nothing to disclose"
        );
        ledger.record(
            "team",
            Some("team"),
            SegmentResolution::Moved(segment("t-1", "sounds-global")),
        );
        ledger.record(
            "build",
            None,
            SegmentResolution::Canonical(segment("b-12", "12")),
        );
        assert!(!ledger.disclosable(&authorized));
        authorized.record("team", "member", "t-1");
        assert!(
            !ledger.disclosable(&authorized),
            "the build is not affirmed yet"
        );
        ledger.affirm("build");
        assert!(ledger.disclosable(&authorized));
    }

    #[test]
    fn a_grant_affirms_only_in_the_namespace_its_resolver_declared() {
        let ledger = SegmentLedger::default();
        let authorized = AuthorizedObjects::default();
        ledger.record(
            "team",
            Some("team"),
            SegmentResolution::Canonical(segment("x-1", "a")),
        );
        ledger.record(
            "build",
            None,
            SegmentResolution::Canonical(segment("x-2", "1")),
        );
        authorized.record("project", "reader", "x-1");
        authorized.record("team", "member", "x-2");
        assert!(
            !ledger.disclosable(&authorized),
            "a grant in another namespace, or on a resolver naming none, affirms nothing"
        );
        authorized.record("team", "member", "x-1");
        ledger.affirm("build");
        assert!(ledger.disclosable(&authorized));
    }

    #[test]
    fn an_unknown_segment_is_never_disclosable() {
        let ledger = SegmentLedger::default();
        ledger.record("team", Some("team"), SegmentResolution::Unknown);
        ledger.affirm("team");
        assert!(!ledger.disclosable(&AuthorizedObjects::default()));
    }

    #[test]
    fn language_paths_spell_each_segment_per_language_and_keep_other_captures() {
        let ledger = SegmentLedger::default();
        let english = "en".parse().expect("tag");
        ledger.record_requested("tab", "files");
        let product = segment("p-1", "eiken-tafel")
            .with_spelling(english, "oak-table")
            .expect("valid");
        ledger.record("product", None, SegmentResolution::Canonical(product));
        let paths = ledger
            .language_paths("/products/{product}/{tab}")
            .expect("every capture has a value");
        assert_eq!(paths.default_path(), "/products/eiken-tafel/files");
        assert_eq!(
            paths.path_in(&"en".parse().expect("tag")),
            "/products/oak-table/files"
        );
        assert_eq!(ledger.requested("tab").as_deref(), Some("files"));
    }
}
