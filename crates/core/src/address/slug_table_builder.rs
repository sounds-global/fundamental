//! [`SlugTableBuilder`] — collects page declarations and refuses ambiguous ones.

#[cfg(not(feature = "std"))]
use alloc::{borrow::ToOwned, string::String, vec::Vec};

use crate::address::slug_table::{SlugAlias, SlugChild};
use crate::address::{AddressError, PageSlug, SlugTable};

/// One page's declaration, as given.
struct Declaration {
    canonical_path: String,
    slugs: Vec<PageSlug>,
    /// A page or its directory, rather than a word no page file names.
    page: bool,
}

/// Collects page slug declarations, then validates them into a [`SlugTable`].
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::{AddressError, PageSlug, SlugTable};
///
/// // `/trust` exists, so `/why` may not claim `trust` as its Dutch address —
/// // `/nl/trust` would otherwise be two pages.
/// let refused = SlugTable::builder()
///     .page("/why", &[PageSlug::current("nl", "trust")])
///     .page("/trust", &[])
///     .build();
/// assert!(matches!(refused, Err(AddressError::SlugShadowsSibling { .. })));
/// ```
#[derive(Default)]
pub struct SlugTableBuilder {
    declarations: Vec<Declaration>,
}

impl SlugTableBuilder {
    /// Declares the slugs for the page registered at `canonical_path`.
    #[must_use]
    pub fn page(mut self, canonical_path: &str, slugs: &[PageSlug]) -> Self {
        self.declarations.push(Declaration {
            canonical_path: canonical_path.to_owned(),
            slugs: slugs.to_vec(),
            page: true,
        });
        self
    }

    /// Declares the slugs of a word in the path that no page file names — a step of a
    /// form at `{step}`, an action beside a page, a document's file stem — under the
    /// template `parent` (`/{team}/aanvragen/{build}`).
    #[must_use]
    pub fn with_segment(mut self, parent: &str, word: &str, slugs: &[PageSlug]) -> Self {
        let mut canonical_path = parent.trim_end_matches('/').to_owned();
        canonical_path.push('/');
        canonical_path.push_str(word);
        self.declarations.push(Declaration {
            canonical_path,
            slugs: slugs.to_vec(),
            page: false,
        });
        self
    }

    /// Declares every word beneath the page or directory at `parent`, as a `slugs!`
    /// declaration's `WORD_SLUGS` holds them ([`with_segment`](Self::with_segment) for each).
    #[must_use]
    pub fn with_segments(self, parent: &str, words: &[(&str, &[PageSlug])]) -> Self {
        words.iter().fold(self, |builder, (word, slugs)| {
            builder.with_segment(parent, word, slugs)
        })
    }

    /// Validates every declaration and builds the table.
    ///
    /// # Errors
    /// - [`AddressError::SlugNotOneSegment`] — a slug that is empty or escapes its
    ///   path segment.
    /// - [`AddressError::SlugOnRootPage`] — `/` has no segment of its own.
    /// - [`AddressError::SlugOnCapture`] — a capture's segment is its caller's value.
    /// - [`AddressError::SlugCollision`] — two siblings claiming one address.
    /// - [`AddressError::SlugShadowsSibling`] — a slug that is a sibling's stem,
    ///   a directory with no page of its own included.
    pub fn build(self) -> Result<SlugTable, AddressError> {
        // Every fixed segment and capture, so a slug is checked against its siblings' addresses.
        let mut statics: Vec<SlugChild> = Vec::new();
        let mut captures: Vec<String> = Vec::new();
        for declaration in &self.declarations {
            add_path(&declaration.canonical_path, &mut statics, &mut captures);
        }

        let mut aliases: Vec<SlugAlias> = Vec::new();
        for declaration in &self.declarations {
            for slug in &declaration.slugs {
                let alias = alias(declaration, slug, &statics, &aliases)?;
                aliases.push(alias);
            }
        }

        Ok(SlugTable::from_parts(aliases, statics, captures))
    }
}

/// Records every fixed segment and capture on `path`'s way down.
fn add_path(path: &str, statics: &mut Vec<SlugChild>, captures: &mut Vec<String>) {
    let mut scope = String::new();
    let mut template = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        template.push('/');
        template.push_str(segment);
        if !is_capture(segment) {
            statics.push(SlugChild {
                scope: scope.clone(),
                stem: segment.to_owned(),
                template: template.clone(),
            });
            scope.push('/');
            scope.push_str(segment);
            continue;
        }
        if !segment.starts_with("{*") {
            captures.push(scope.clone());
        }
        scope.push_str("/{}");
    }
}

/// Validates one slug of one declaration against the tree and the aliases so far.
fn alias(
    declaration: &Declaration,
    slug: &PageSlug,
    statics: &[SlugChild],
    aliases: &[SlugAlias],
) -> Result<SlugAlias, AddressError> {
    let (parent, stem) = split_parent(&declaration.canonical_path);
    let scope = scope_of(parent);
    check(declaration, stem, slug)?;
    // A *different* sibling's stem; aliasing its own stem is harmless.
    let shadowed = statics
        .iter()
        .find(|child| child.scope == scope && child.stem == slug.slug() && child.stem != stem);
    if let Some(shadowed) = shadowed {
        return Err(AddressError::SlugShadowsSibling {
            parent: parent.to_owned(),
            language: slug.language().to_owned(),
            slug: slug.slug().to_owned(),
            shadowed: shadowed.template.clone(),
        });
    }
    // Retired spellings too: one colliding with a live one would redirect a working address.
    let collides = aliases.iter().any(|existing| {
        existing.parent == scope
            && existing.language == slug.language()
            && existing.localized == slug.slug()
            && existing.canonical != stem
    });
    if collides {
        return Err(AddressError::SlugCollision {
            parent: parent.to_owned(),
            language: slug.language().to_owned(),
            slug: slug.slug().to_owned(),
        });
    }
    Ok(SlugAlias {
        parent: scope,
        language: slug.language().to_owned(),
        localized: slug.slug().to_owned(),
        canonical: stem.to_owned(),
        current: slug.is_current(),
        page: declaration.page,
    })
}

/// Refuses a slug that cannot be one: malformed, on the root, or on a capture.
fn check(declaration: &Declaration, stem: &str, slug: &PageSlug) -> Result<(), AddressError> {
    if !slug.is_one_segment() {
        return Err(AddressError::SlugNotOneSegment {
            page: declaration.canonical_path.clone(),
            slug: slug.slug().to_owned(),
        });
    }
    if stem.is_empty() {
        return Err(AddressError::SlugOnRootPage {
            language: slug.language().to_owned(),
            slug: slug.slug().to_owned(),
        });
    }
    if is_capture(stem) {
        return Err(AddressError::SlugOnCapture {
            page: declaration.canonical_path.clone(),
            language: slug.language().to_owned(),
            slug: slug.slug().to_owned(),
        });
    }
    Ok(())
}

/// Whether a template segment is a capture: `{name}` or `{*name}`.
fn is_capture(segment: &str) -> bool {
    segment.starts_with('{') && segment.ends_with('}')
}

/// The scope a template parent names: every capture written `{}`, so `/{team}/x` and
/// `/{org}/x` are one place in the tree.
fn scope_of(parent: &str) -> String {
    let mut scope = String::with_capacity(parent.len());
    for segment in parent.split('/').filter(|s| !s.is_empty()) {
        scope.push('/');
        scope.push_str(match is_capture(segment) {
            true => "{}",
            false => segment,
        });
    }
    scope
}

/// Splits a canonical path into `(parent, stem)`.
fn split_parent(canonical_path: &str) -> (&str, &str) {
    let trimmed = canonical_path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(at) => (&trimmed[..at], &trimmed[at + 1..]),
        None => ("", trimmed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_parent_names_its_scope() {
        assert_eq!(scope_of(""), "");
        assert_eq!(scope_of("/{team}/aanvragen/{build}"), "/{}/aanvragen/{}");
        assert_eq!(scope_of("/{org}/aanvragen/{id}"), "/{}/aanvragen/{}");
    }

    #[test]
    fn a_path_splits_into_its_parent_and_stem() {
        assert_eq!(split_parent("/why"), ("", "why"));
        assert_eq!(split_parent("/guides/api"), ("/guides", "api"));
        assert_eq!(split_parent("/a/b/c"), ("/a/b", "c"));
        assert_eq!(split_parent("/"), ("", ""));
        assert_eq!(split_parent("/guides/"), ("", "guides"));
    }

    #[test]
    fn two_siblings_claiming_one_slug_in_one_language_are_refused() {
        let error = SlugTable::builder()
            .page("/why", &[PageSlug::current("nl", "waarom")])
            .page("/how", &[PageSlug::current("nl", "waarom")])
            .build()
            .expect_err("one address cannot serve two pages");
        assert!(
            matches!(error, AddressError::SlugCollision { .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_slug_that_shadows_a_sibling_stem_is_refused() {
        let error = SlugTable::builder()
            .page("/why", &[PageSlug::current("nl", "trust")])
            .page("/trust", &[])
            .build()
            .expect_err("/nl/trust would mean two pages");
        assert!(
            matches!(error, AddressError::SlugShadowsSibling { .. }),
            "got {error:?}"
        );
    }

    /// Declaration order must not decide.
    #[test]
    fn shadowing_is_refused_whichever_page_is_declared_first() {
        for pages in [0, 1] {
            let builder = SlugTable::builder();
            let builder = if pages == 0 {
                builder
                    .page("/trust", &[])
                    .page("/why", &[PageSlug::current("nl", "trust")])
            } else {
                builder
                    .page("/why", &[PageSlug::current("nl", "trust")])
                    .page("/trust", &[])
            };
            assert!(
                matches!(
                    builder.build(),
                    Err(AddressError::SlugShadowsSibling { .. })
                ),
                "declaration order {pages} was accepted"
            );
        }
    }

    /// Pointless, but not ambiguous.
    #[test]
    fn a_page_may_name_its_own_stem() {
        assert!(
            SlugTable::builder()
                .page("/trust", &[PageSlug::current("nl", "trust")])
                .build()
                .is_ok()
        );
    }

    #[test]
    fn a_shadow_only_applies_between_siblings() {
        // Different parents, different addresses.
        assert!(
            SlugTable::builder()
                .page("/trust", &[])
                .page("/legal", &[])
                .page("/legal/why", &[PageSlug::current("nl", "trust")])
                .build()
                .is_ok()
        );
    }

    #[test]
    fn a_retired_slug_may_not_take_a_live_address_from_another_page() {
        let error = SlugTable::builder()
            .page("/why", &[PageSlug::current("nl", "waarom")])
            .page("/how", &[PageSlug::retired("nl", "waarom")])
            .build()
            .expect_err("a retired spelling must not redirect a live address away");
        assert!(matches!(error, AddressError::SlugCollision { .. }));
    }

    #[test]
    fn a_page_may_retire_and_replace_its_own_slug() {
        let table = SlugTable::builder()
            .page(
                "/why",
                &[
                    PageSlug::current("nl", "waarom"),
                    PageSlug::retired("nl", "waarom-acme"),
                ],
            )
            .build()
            .expect("one page owning both spellings is the whole point");
        assert_eq!(table.de_alias("/waarom-acme", "nl"), "/why");
        assert_eq!(table.localize("/why", "nl"), "/waarom");
    }

    #[test]
    fn a_malformed_slug_is_refused() {
        for bad in ["", "a/b", "a?b", "a#b"] {
            let error = SlugTable::builder()
                .page("/why", &[PageSlug::current("nl", bad)])
                .build()
                .expect_err("'{bad}' escapes its segment");
            assert!(matches!(error, AddressError::SlugNotOneSegment { .. }));
        }
    }

    #[test]
    fn the_root_page_cannot_declare_a_slug() {
        let error = SlugTable::builder()
            .page("/", &[PageSlug::current("nl", "start")])
            .build()
            .expect_err("'/' has no segment to translate");
        assert!(matches!(error, AddressError::SlugOnRootPage { .. }));
    }

    #[test]
    fn two_languages_may_use_the_same_word_for_one_page() {
        let table = SlugTable::builder()
            .page(
                "/trust",
                &[
                    PageSlug::current("nl", "vertrouwen"),
                    PageSlug::current("de", "vertrauen"),
                ],
            )
            .build()
            .expect("different languages are different keys");
        assert_eq!(table.de_alias("/vertrouwen", "nl"), "/trust");
        assert_eq!(table.de_alias("/vertrauen", "de"), "/trust");
        assert_eq!(
            table.de_alias("/vertrauen", "nl"),
            "/vertrauen",
            "a German spelling is not a Dutch address"
        );
    }

    #[test]
    fn a_slug_on_a_capture_is_refused() {
        for page in ["/{team}", "/{team}/aanvragen/{build}", "/docs/{*rest}"] {
            let error = SlugTable::builder()
                .page(page, &[PageSlug::current("en", "team")])
                .build()
                .expect_err("a capture's value is the caller's");
            assert!(
                matches!(&error, AddressError::SlugOnCapture { page: named, .. } if named == page),
                "got {error:?}"
            );
            assert_eq!(error.code(), "ADDR_012");
        }
    }

    #[test]
    fn a_slug_may_not_shadow_a_directory_with_no_page() {
        let error = SlugTable::builder()
            .page("/bestand/verlopen", &[])
            .page("/files", &[PageSlug::current("en", "bestand")])
            .build()
            .expect_err("/en/bestand would mean two pages");
        assert!(
            matches!(&error, AddressError::SlugShadowsSibling { shadowed, .. } if shadowed == "/bestand"),
            "got {error:?}"
        );
    }

    /// `/{team}/x` and `/{org}/y` sit at one place in the tree.
    #[test]
    fn two_captures_claiming_one_word_collide() {
        let error = SlugTable::builder()
            .page("/{team}/x", &[PageSlug::current("en", "same")])
            .page("/{org}/y", &[PageSlug::current("en", "same")])
            .build()
            .expect_err("one address cannot serve two pages");
        assert!(
            matches!(error, AddressError::SlugCollision { .. }),
            "got {error:?}"
        );
    }
}
