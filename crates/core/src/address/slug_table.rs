//! [`SlugTable`] — every page's path segment in every language it declares one in.

#[cfg(not(feature = "std"))]
use alloc::{
    borrow::{Cow, ToOwned},
    string::String,
    vec::Vec,
};
#[cfg(feature = "std")]
use std::borrow::Cow;

use crate::address::{SlugTableBuilder, percent_decode_segment, percent_encode_segment};

/// One page segment written one way in one language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SlugAlias {
    /// Its parent's template, every capture written `{}` (`""` at the root).
    pub(super) parent: String,
    /// The primary language subtag.
    pub(super) language: String,
    /// The segment as written in that language.
    pub(super) localized: String,
    /// The segment as the route table knows it.
    pub(super) canonical: String,
    /// Whether this spelling may be emitted, or only accepted.
    pub(super) current: bool,
    /// Whether a page declared it, rather than [`SlugTableBuilder::with_segment`].
    pub(super) page: bool,
}

/// A fixed segment of the route tree: a page, a directory, or a word no page names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SlugChild {
    /// The scope it sits in, as [`SlugAlias::parent`].
    pub(super) scope: String,
    /// The segment as the route table knows it.
    pub(super) stem: String,
    /// Its path as declared, captures named — what a report shows.
    pub(super) template: String,
}

/// Translates a request path into the route table's vocabulary, and back.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::{PageSlug, SlugTable};
///
/// let table = SlugTable::builder()
///     .page("/why", &[PageSlug::current("nl", "waarom")])
///     .page("/guides", &[PageSlug::current("nl", "gidsen")])
///     .page("/guides/getting-started", &[PageSlug::current("nl", "aan-de-slag")])
///     .build()?;
///
/// // Inbound: a Dutch address becomes the route the table holds.
/// assert_eq!(table.de_alias("/gidsen/aan-de-slag", "nl"), "/guides/getting-started");
/// // Outbound: the route becomes the Dutch address again.
/// assert_eq!(table.localize("/guides/getting-started", "nl"), "/gidsen/aan-de-slag");
/// // A language that declared nothing reads the canonical stems, never a half-
/// // translated mixture.
/// assert_eq!(table.localize("/guides/getting-started", "de"), "/guides/getting-started");
/// # Ok::<(), fundamental_core::address::AddressError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlugTable {
    /// Sorted by `(parent, language, localized)` — the inbound lookup.
    inbound: Vec<SlugAlias>,
    /// Indices into `inbound` sorted by `(parent, canonical, language)`, current entries only:
    /// the outbound lookup.
    outbound: Vec<usize>,
    /// Every fixed segment of the route tree, sorted by `(scope, stem)`: each declared
    /// page, each directory above one, and each word no page names.
    statics: Vec<SlugChild>,
    /// Every scope a one-segment capture (`{name}`) sits in, sorted. A catch-all is not
    /// here: everything beneath one is the caller's, so the walk stops at it.
    captures: Vec<String>,
}

impl SlugTable {
    /// Starts building a table.
    #[must_use]
    pub fn builder() -> SlugTableBuilder {
        SlugTableBuilder::default()
    }

    /// Builds a table from validated aliases and the pages they were declared on.
    pub(super) fn from_parts(
        mut inbound: Vec<SlugAlias>,
        mut statics: Vec<SlugChild>,
        mut captures: Vec<String>,
    ) -> Self {
        inbound.sort_by(|a, b| {
            (a.parent.as_str(), a.language.as_str(), a.localized.as_str()).cmp(&(
                b.parent.as_str(),
                b.language.as_str(),
                b.localized.as_str(),
            ))
        });
        let mut outbound: Vec<usize> = (0..inbound.len()).filter(|i| inbound[*i].current).collect();
        outbound.sort_by(|&a, &b| {
            let (a, b) = (&inbound[a], &inbound[b]);
            (a.parent.as_str(), a.canonical.as_str(), a.language.as_str()).cmp(&(
                b.parent.as_str(),
                b.canonical.as_str(),
                b.language.as_str(),
            ))
        });
        statics.sort_by(|a, b| {
            (a.scope.as_str(), a.stem.as_str()).cmp(&(b.scope.as_str(), b.stem.as_str()))
        });
        statics.dedup_by(|a, b| a.scope == b.scope && a.stem == b.stem);
        captures.sort();
        captures.dedup();
        Self {
            inbound,
            outbound,
            statics,
            captures,
        }
    }

    /// Every spelling a page is reached by in some language, current or retired — the words a
    /// dynamic segment beside these pages can never take.
    pub fn spellings(&self) -> impl Iterator<Item = &str> {
        self.inbound
            .iter()
            .filter(|alias| alias.page)
            .map(|alias| alias.localized.as_str())
    }

    /// The pages `language` does **not** spell, as canonical paths.
    ///
    /// The canonical language is not special-cased: no page declares it, so it is missing
    /// every page.
    #[must_use]
    pub fn unspelled(&self, language: &str) -> Vec<String> {
        self.statics
            .iter()
            .filter(|child| {
                self.find_outbound(&child.scope, &child.stem, language)
                    .is_none()
            })
            .map(|child| child.template.clone())
            .collect()
    }

    /// Whether any page declared a slug.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inbound.is_empty()
    }

    /// Rewrites a request path into the route table's canonical vocabulary.
    ///
    /// `path` is percent-encoded; each segment is decoded only to look it up, so a `%2F`
    /// cannot become a separator.
    #[must_use]
    pub fn de_alias<'a>(&self, path: &'a str, language: &str) -> Cow<'a, str> {
        self.walk(path, language, Direction::Inbound)
    }

    /// Rewrites a canonical path into how `language` writes it.
    ///
    /// Retired spellings are never produced.
    #[must_use]
    pub fn localize<'a>(&self, canonical_path: &'a str, language: &str) -> Cow<'a, str> {
        self.walk(canonical_path, language, Direction::Outbound)
    }

    /// The shared walk: translate each segment, accumulating the scope for the next one.
    fn walk<'a>(&self, path: &'a str, language: &str, direction: Direction) -> Cow<'a, str> {
        if self.inbound.is_empty() {
            return Cow::Borrowed(path);
        }
        let mut scope = Some(String::new());
        let mut out = String::new();
        let mut changed = false;

        for segment in path.split('/').filter(|s| !s.is_empty()) {
            out.push('/');
            let Some(at) = scope.as_mut() else {
                out.push_str(segment);
                continue;
            };
            match self.step(at, segment, language, direction) {
                Step::Spelled { emit, canonical } => {
                    changed |= emit != segment;
                    out.push_str(&emit);
                    at.push('/');
                    at.push_str(&canonical);
                }
                Step::Fixed => {
                    out.push_str(segment);
                    at.push('/');
                    at.push_str(segment);
                }
                Step::Value => {
                    out.push_str(segment);
                    at.push_str("/{}");
                }
                Step::Rest => {
                    out.push_str(segment);
                    scope = None;
                }
            }
        }

        if !changed {
            return Cow::Borrowed(path);
        }
        if out.is_empty() {
            out.push('/');
        }
        Cow::Owned(out)
    }

    /// Reads one segment at `scope`.
    fn step(&self, scope: &str, segment: &str, language: &str, direction: Direction) -> Step {
        let spelled = match direction {
            // Only the key is decoded: an unmatched segment leaves as it arrived, so an escaped
            // separator stays escaped.
            Direction::Inbound => self
                .find_inbound(scope, language, percent_decode_segment(segment).as_ref())
                .map(|alias| Step::Spelled {
                    emit: alias.canonical.clone(),
                    canonical: alias.canonical.clone(),
                }),
            Direction::Outbound => {
                self.find_outbound(scope, segment, language)
                    .map(|alias| Step::Spelled {
                        emit: percent_encode_segment(&alias.localized).into_owned(),
                        canonical: segment.to_owned(),
                    })
            }
        };
        if let Some(step) = spelled {
            return step;
        }
        if self.is_static(scope, segment) {
            return Step::Fixed;
        }
        if self
            .captures
            .binary_search_by(|at| at.as_str().cmp(scope))
            .is_ok()
        {
            return Step::Value;
        }
        Step::Rest
    }

    fn is_static(&self, scope: &str, stem: &str) -> bool {
        self.statics
            .binary_search_by(|child| {
                (child.scope.as_str(), child.stem.as_str()).cmp(&(scope, stem))
            })
            .is_ok()
    }

    fn find_inbound(&self, parent: &str, language: &str, localized: &str) -> Option<&SlugAlias> {
        self.inbound
            .binary_search_by(|alias| {
                (
                    alias.parent.as_str(),
                    alias.language.as_str(),
                    alias.localized.as_str(),
                )
                    .cmp(&(parent, language, localized))
            })
            .ok()
            .and_then(|at| self.inbound.get(at))
    }

    fn find_outbound(&self, parent: &str, canonical: &str, language: &str) -> Option<&SlugAlias> {
        self.outbound
            .binary_search_by(|&index| {
                let alias = &self.inbound[index];
                (
                    alias.parent.as_str(),
                    alias.canonical.as_str(),
                    alias.language.as_str(),
                )
                    .cmp(&(parent, canonical, language))
            })
            .ok()
            .and_then(|at| self.outbound.get(at))
            .and_then(|&index| self.inbound.get(index))
    }
}

/// What the walk made of one segment.
enum Step {
    /// A declared spelling: `emit` is written, `canonical` extends the scope.
    Spelled { emit: String, canonical: String },
    /// A fixed segment with no spelling here, kept as it is.
    Fixed,
    /// A capture's value, kept as it is.
    Value,
    /// A catch-all, or no place in the tree: this and the rest are kept as they are.
    Rest,
}

/// Which way a path is being translated.
#[derive(Debug, Clone, Copy)]
enum Direction {
    Inbound,
    Outbound,
}

#[cfg(test)]
mod coverage {
    //! Which languages spell every page.

    use super::SlugTable;
    use crate::address::PageSlug;

    fn table() -> SlugTable {
        SlugTable::builder()
            .page("/", &[])
            .page("/why", &[PageSlug::current("nl", "waarom")])
            .page(
                "/pricing",
                &[
                    PageSlug::current("nl", "prijzen"),
                    PageSlug::current("pt", "precos"),
                ],
            )
            .build()
            .expect("a well-formed table")
    }

    #[test]
    fn a_half_translated_language_names_the_pages_it_is_missing() {
        assert_eq!(table().unspelled("pt"), ["/why"]);
    }

    #[test]
    fn a_language_that_spells_every_page_is_complete() {
        assert!(table().unspelled("nl").is_empty());
    }

    /// The root has no segment, so reporting it would leave no language complete.
    #[test]
    fn the_root_is_not_a_page_that_needs_spelling() {
        for language in ["nl", "pt", "sw"] {
            assert!(
                !table().unspelled(language).iter().any(|page| page == "/"),
                "`/` was reported as unspelled in {language}"
            );
        }
    }

    #[test]
    fn an_undeclared_language_is_missing_everything_including_the_canonical_one() {
        let table = table();
        assert_eq!(table.unspelled("sw").len(), 2);
        assert_eq!(
            table.unspelled("en").len(),
            2,
            "the canonical language is not special-cased here"
        );
    }

    #[test]
    fn a_retired_spelling_is_not_an_address() {
        let table = SlugTable::builder()
            .page("/why", &[PageSlug::retired("nl", "waarom")])
            .build()
            .expect("a well-formed table");
        assert_eq!(table.unspelled("nl"), ["/why"]);
    }
    #[test]
    fn a_capture_is_not_a_page_that_needs_spelling() {
        let table = SlugTable::builder()
            .page("/{team}", &[])
            .page("/{team}/aanvragen/{build}", &[])
            .page("/docs/{*rest}", &[])
            .build()
            .expect("a well-formed table");
        assert_eq!(table.unspelled("en"), ["/docs", "/{team}/aanvragen"]);
    }

    #[test]
    fn a_directory_with_no_page_still_needs_its_spelling() {
        let table = SlugTable::builder()
            .page("/bestand/verlopen", &[PageSlug::current("en", "expired")])
            .build()
            .expect("a well-formed table");
        assert_eq!(table.unspelled("en"), ["/bestand"]);
    }

    #[test]
    fn a_page_beneath_a_capture_is_reported_by_its_template() {
        let table = SlugTable::builder()
            .page("/{team}/meldingen", &[])
            .page("/{team}/facturen", &[PageSlug::current("en", "invoices")])
            .build()
            .expect("a well-formed table");
        assert_eq!(table.unspelled("en"), ["/{team}/meldingen"]);
    }
}

#[cfg(test)]
mod beneath_captures {
    //! Pages beneath a Team's handle, spelled in English over Dutch canonical paths.

    use super::*;
    use crate::address::PageSlug;

    fn en(slug: &'static str) -> [PageSlug; 1] {
        [PageSlug::current("en", slug)]
    }

    /// Every template the fixture declares, beside the values a request puts in its captures.
    const TEMPLATES: [&str; 14] = [
        "/aanmelden",
        "/aanvraag/{step}",
        "/{team}",
        "/{team}/aanvragen",
        "/{team}/aanvragen/{build}",
        "/{team}/aanvragen/{build}/{step}",
        "/{team}/aanvragen/{build}/bouwbrief",
        "/{team}/aanvragen/{build}/bouwbrief/ondertekenen",
        "/{team}/facturen",
        "/{team}/facturen/{number}/betaling",
        "/bestand/verlopen",
        "/staff/bouwbrieven/{team}/{build}/medeondertekenen",
        "/docs/{*rest}",
        "/{org}/leden",
    ];

    fn table() -> SlugTable {
        SlugTable::builder()
            .page("/", &[])
            .page(
                "/aanmelden",
                &[
                    PageSlug::current("en", "sign-in"),
                    PageSlug::current("de", "anmelden"),
                ],
            )
            .page("/aanvraag/{step}", &[])
            .with_segment("/aanvraag", "bedrijf", &en("company"))
            .page("/{team}", &[])
            .page("/{team}/aanvragen", &en("build-requests"))
            .page("/{team}/aanvragen/{build}", &[])
            .page("/{team}/aanvragen/{build}/{step}", &[])
            .with_segment("/{team}/aanvragen/{build}", "bedrijf", &en("company"))
            .page("/{team}/aanvragen/{build}/bouwbrief", &en("build-brief"))
            .page(
                "/{team}/aanvragen/{build}/bouwbrief/ondertekenen",
                &en("sign"),
            )
            .page("/{team}/facturen", &en("invoices"))
            .page("/{team}/facturen/{number}/betaling", &en("payment"))
            .page("/bestand", &en("file"))
            .page("/bestand/verlopen", &en("expired"))
            .page("/staff/bouwbrieven", &en("build-briefs"))
            .page(
                "/staff/bouwbrieven/{team}/{build}/medeondertekenen",
                &en("countersign"),
            )
            .page("/docs/{*rest}", &[])
            .page("/{org}/leden", &en("members"))
            .build()
            .expect("the fixture table is well formed")
    }

    #[test]
    fn a_segment_beneath_a_capture_translates_both_ways() {
        let table = table();
        assert_eq!(
            table.de_alias("/acme/build-requests/42/build-brief/sign", "en"),
            "/acme/aanvragen/42/bouwbrief/ondertekenen"
        );
        assert_eq!(
            table.localize("/acme/aanvragen/42/bouwbrief/ondertekenen", "en"),
            "/acme/build-requests/42/build-brief/sign"
        );
    }

    #[test]
    fn a_directory_with_no_page_scopes_its_children() {
        let table = table();
        assert_eq!(
            table.de_alias("/acme/invoices/7/payment", "en"),
            "/acme/facturen/7/betaling"
        );
        assert_eq!(
            table.localize("/staff/bouwbrieven/acme/42/medeondertekenen", "en"),
            "/staff/build-briefs/acme/42/countersign"
        );
    }

    /// The router prefers a fixed segment to a capture, and so does the walk.
    #[test]
    fn a_value_spelled_like_a_static_sibling_is_read_as_the_sibling() {
        let table = table();
        assert_eq!(
            table.de_alias("/sign-in/build-requests", "en"),
            "/aanmelden/build-requests",
            "`/aanmelden` has no child `aanvragen`, so the walk stops translating"
        );
        assert_eq!(
            table.de_alias("/staff/build-briefs/acme/42/countersign", "en"),
            "/staff/bouwbrieven/acme/42/medeondertekenen"
        );
    }

    #[test]
    fn a_spelling_in_another_language_is_a_value() {
        assert_eq!(
            table().de_alias("/anmelden/build-requests", "en"),
            "/anmelden/aanvragen",
            "a German spelling is a Team's handle on an English reading"
        );
    }

    #[test]
    fn a_catch_all_stops_translation() {
        let table = table();
        for path in ["/docs/sign-in/build-requests", "/docs/x/invoices"] {
            assert_eq!(table.de_alias(path, "en"), path);
            assert_eq!(table.localize(path, "en"), path);
        }
    }

    #[test]
    fn two_captures_at_one_place_share_one_scope() {
        let table = table();
        assert_eq!(table.de_alias("/acme/members", "en"), "/acme/leden");
        assert_eq!(
            table.de_alias("/acme/build-requests", "en"),
            "/acme/aanvragen"
        );
    }

    #[test]
    fn an_escaped_separator_beneath_a_capture_cannot_escape_its_segment() {
        let table = table();
        for (attempt, expected) in [
            ("/acme%2Fbuild-requests/42", "/acme%2Fbuild-requests/42"),
            (
                "/acme/build-requests/%2E%2E%2Fsign",
                "/acme/aanvragen/%2E%2E%2Fsign",
            ),
            (
                "/acme/build-requests/42/build-brief%2Fsign",
                "/acme/aanvragen/42/build-brief%2Fsign",
            ),
        ] {
            let resolved = table.de_alias(attempt, "en");
            assert_eq!(resolved, expected);
            assert_eq!(
                resolved.split('/').filter(|s| !s.is_empty()).count(),
                attempt.split('/').filter(|s| !s.is_empty()).count(),
                "{attempt} gained a path segment"
            );
        }
    }

    /// A Team may still take such a word as a handle.
    #[test]
    fn a_word_no_page_names_is_spelled_at_its_scope_only() {
        let table = table();
        assert_eq!(
            table.de_alias("/aanvraag/company", "en"),
            "/aanvraag/bedrijf"
        );
        assert_eq!(
            table.localize("/acme/aanvragen/42/bedrijf", "en"),
            "/acme/build-requests/42/company"
        );
        assert_eq!(
            table.de_alias("/company/build-requests", "en"),
            "/company/aanvragen",
            "at the root the word is a Team's handle"
        );
        assert!(
            !table.spellings().any(|word| word == "company"),
            "a segment word is not reserved from Team handles"
        );
    }

    /// Every template, captures filled, survives English and back with no Dutch stem left.
    #[test]
    fn every_template_round_trips_beneath_its_captures() {
        let table = table();
        let dutch: Vec<&str> = table
            .inbound
            .iter()
            .filter(|alias| alias.language == "en")
            .map(|alias| alias.canonical.as_str())
            .collect();
        for template in TEMPLATES {
            let canonical = template
                .replace("{*rest}", "a/b")
                .replace("{step}", "bedrijf")
                .replace("{team}", "acme")
                .replace("{org}", "acme")
                .replace("{build}", "42")
                .replace("{number}", "7");
            let localized = table.localize(&canonical, "en");
            assert_eq!(
                table.de_alias(&localized, "en"),
                canonical,
                "{template} did not survive the trip through {localized}"
            );
            for segment in localized.split('/') {
                assert!(
                    !dutch.contains(&segment),
                    "{template} kept the Dutch word '{segment}' in {localized}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::PageSlug;

    fn table() -> SlugTable {
        SlugTable::builder()
            .page("/why", &[PageSlug::current("nl", "waarom")])
            .page("/trust", &[])
            .page("/guides", &[PageSlug::current("nl", "gidsen")])
            .page(
                "/guides/getting-started",
                &[
                    PageSlug::current("nl", "aan-de-slag"),
                    PageSlug::retired("nl", "beginnen"),
                ],
            )
            .build()
            .expect("the fixture table is well formed")
    }

    #[test]
    fn a_localized_path_de_aliases_to_the_route_the_table_holds() {
        assert_eq!(table().de_alias("/waarom", "nl"), "/why");
        assert_eq!(
            table().de_alias("/gidsen/aan-de-slag", "nl"),
            "/guides/getting-started"
        );
    }

    #[test]
    fn a_canonical_path_localizes_back_to_the_same_address() {
        let table = table();
        for canonical in ["/why", "/guides", "/guides/getting-started"] {
            let localized = table.localize(canonical, "nl");
            assert_eq!(
                table.de_alias(&localized, "nl"),
                canonical,
                "{canonical} did not survive the round trip through {localized}"
            );
        }
    }

    #[test]
    fn a_directory_slug_localizes_every_page_beneath_it() {
        assert_eq!(
            table().localize("/guides/getting-started", "nl"),
            "/gidsen/aan-de-slag"
        );
    }

    #[test]
    fn an_undeclared_language_reads_the_canonical_stems() {
        assert_eq!(
            table().localize("/guides/getting-started", "de"),
            "/guides/getting-started"
        );
        assert_eq!(
            table().de_alias("/guides/getting-started", "de"),
            "/guides/getting-started"
        );
    }

    /// The fallback is per segment, not per path.
    #[test]
    fn a_page_with_no_slug_keeps_its_stem_inside_a_translated_parent() {
        let table = SlugTable::builder()
            .page("/guides", &[PageSlug::current("nl", "gidsen")])
            .page("/guides/api", &[])
            .build()
            .expect("well formed");
        assert_eq!(table.localize("/guides/api", "nl"), "/gidsen/api");
        assert_eq!(table.de_alias("/gidsen/api", "nl"), "/guides/api");
    }

    #[test]
    fn a_retired_slug_is_accepted_inbound_and_never_emitted_outbound() {
        let table = table();
        assert_eq!(
            table.de_alias("/gidsen/beginnen", "nl"),
            "/guides/getting-started",
            "an old link must keep resolving"
        );
        assert_eq!(
            table.localize("/guides/getting-started", "nl"),
            "/gidsen/aan-de-slag",
            "but the current spelling is the one address"
        );
    }

    #[test]
    fn the_same_slug_under_two_parents_means_two_pages() {
        let table = SlugTable::builder()
            .page("/guides", &[])
            .page("/legal", &[])
            .page("/guides/overview", &[PageSlug::current("nl", "overzicht")])
            .page("/legal/overview", &[PageSlug::current("nl", "overzicht")])
            .build()
            .expect("siblings under different parents do not collide");
        assert_eq!(
            table.de_alias("/guides/overzicht", "nl"),
            "/guides/overview"
        );
        assert_eq!(table.de_alias("/legal/overzicht", "nl"), "/legal/overview");
    }

    #[test]
    fn an_unchanged_path_is_borrowed_rather_than_rebuilt() {
        let table = table();
        assert!(matches!(table.de_alias("/trust", "nl"), Cow::Borrowed(_)));
        assert!(matches!(table.localize("/trust", "en"), Cow::Borrowed(_)));
        assert!(matches!(table.de_alias("/waarom", "nl"), Cow::Owned(_)));
    }

    #[test]
    fn an_empty_table_translates_nothing_and_allocates_nothing() {
        let table = SlugTable::default();
        assert!(table.is_empty());
        assert!(matches!(table.de_alias("/why", "nl"), Cow::Borrowed(_)));
    }

    #[test]
    fn the_root_path_survives_both_directions() {
        assert_eq!(table().de_alias("/", "nl"), "/");
        assert_eq!(table().localize("/", "nl"), "/");
    }

    /// So `/nl/gidsen/42` reaches `/guides/{id}`.
    #[test]
    fn a_dynamic_segment_passes_through_untouched() {
        assert_eq!(table().de_alias("/gidsen/42", "nl"), "/guides/42");
    }

    /// A vocabulary written in the language's own script.
    fn scripts() -> SlugTable {
        SlugTable::builder()
            .page("/login", &[PageSlug::current("ja", "ログイン")])
            .page("/about", &[PageSlug::current("ru", "вход")])
            .build()
            .expect("the fixture table is well formed")
    }

    #[test]
    fn a_request_in_another_script_resolves_from_the_form_the_wire_carries() {
        assert_eq!(
            scripts().de_alias("/%E3%83%AD%E3%82%B0%E3%82%A4%E3%83%B3", "ja"),
            "/login"
        );
        assert_eq!(
            scripts().de_alias("/%D0%B2%D1%85%D0%BE%D0%B4", "ru"),
            "/about"
        );
    }

    #[test]
    fn the_address_such_a_page_advertises_is_the_one_a_url_can_carry() {
        let localized = scripts().localize("/login", "ja");
        assert_eq!(localized, "/%E3%83%AD%E3%82%B0%E3%82%A4%E3%83%B3");
        assert!(localized.is_ascii(), "a URL carries ASCII");
        assert_eq!(scripts().de_alias(&localized, "ja"), "/login");
    }

    /// The lookup key is decoded; the emitted value never is.
    #[test]
    fn an_escaped_separator_cannot_escape_its_segment() {
        for attempt in [
            "/%2E%2E%2Fetc%2Fpasswd",
            "/gidsen/%2E%2E%2F%2E%2E",
            "/%2Fadmin",
        ] {
            let resolved = scripts().de_alias(attempt, "ja");
            assert_eq!(resolved, attempt, "an unmatched segment is passed through");
            assert_eq!(
                resolved.split('/').filter(|s| !s.is_empty()).count(),
                attempt.split('/').filter(|s| !s.is_empty()).count(),
                "{attempt} gained a path segment"
            );
        }
    }

    /// The address is still the encoded one, so the caller redirects there once.
    #[test]
    fn an_unencoded_request_resolves_and_still_has_one_canonical_address() {
        assert_eq!(scripts().de_alias("/ログイン", "ja"), "/login");
        assert_eq!(
            scripts().localize("/login", "ja"),
            "/%E3%83%AD%E3%82%B0%E3%82%A4%E3%83%B3"
        );
    }

    /// An ASCII vocabulary allocates nothing new.
    #[test]
    fn an_ascii_spelling_is_still_borrowed_on_the_way_out() {
        assert!(matches!(table().localize("/trust", "nl"), Cow::Borrowed(_)));
        assert_eq!(
            table().localize("/guides/getting-started", "nl"),
            "/gidsen/aan-de-slag"
        );
    }
}
