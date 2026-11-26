//! [`AddressError`] — why an address space could not be built.

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
use core::fmt;
#[cfg(feature = "std")]
use std::borrow::Cow;

/// Why a [`LocalePrefixSet`](crate::address::LocalePrefixSet) or
/// [`SlugTable`](crate::address::SlugTable) could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddressError {
    /// A prefix was empty, or contained a `/`.
    PrefixNotOneSegment {
        /// The offending prefix, as given.
        prefix: String,
    },

    /// Two entries claimed the same prefix.
    DuplicatePrefix {
        /// The prefix claimed twice.
        prefix: String,
    },

    /// A slug was empty, or carried a character that is not part of a path segment.
    SlugNotOneSegment {
        /// The page whose declaration is wrong.
        page: String,
        /// The offending slug, as given.
        slug: String,
    },

    /// Two sibling pages claimed the same slug in the same language.
    SlugCollision {
        /// The canonical parent path the two pages share.
        parent: String,
        /// The language both declared it in.
        language: String,
        /// The slug both claimed.
        slug: String,
    },

    /// The root page declared a slug.
    SlugOnRootPage {
        /// The language it was declared in.
        language: String,
        /// The slug that has nowhere to go.
        slug: String,
    },

    /// A slug is the canonical stem of a *different* sibling page.
    SlugShadowsSibling {
        /// The canonical parent path the two pages share.
        parent: String,
        /// The language the slug was declared in.
        language: String,
        /// The slug that shadows.
        slug: String,
        /// The sibling page whose canonical address it would take.
        shadowed: String,
    },

    /// A slug was declared on a capture (`[team].rs`, `[team]/index.rs`, `[team]/layout.rs`).
    SlugOnCapture {
        /// The page whose declaration is wrong, as its template.
        page: String,
        /// The language it was declared in.
        language: String,
        /// The slug that has nowhere to go.
        slug: String,
    },

    /// The language unprefixed pages are served in is also served under a prefix.
    UnprefixedLocaleIsPrefixed {
        /// The language, as a tag.
        locale: String,
        /// The prefix already serving it.
        prefix: String,
    },

    /// An origin carried something an origin may not: a path, a query, userinfo, an IP
    /// literal, a malformed label, whitespace.
    OriginInvalid {
        /// The origin, as given.
        origin: String,
        /// What is wrong with it.
        reason: &'static str,
    },

    /// A published origin was not `https`.
    OriginNotHttps {
        /// The origin, as given.
        origin: String,
    },

    /// A default language was declared beside a construct it replaces.
    DefaultLanguageConflicts {
        /// The default language, as a tag.
        locale: String,
        /// The construct it conflicts with.
        with: &'static str,
    },

    /// A language-per-origin setup is inconsistent.
    LanguageOriginsInvalid {
        /// What is inconsistent.
        reason: &'static str,
        /// The value it concerns.
        value: String,
    },
}

impl AddressError {
    /// A stable code for logs and dashboards, unchanged by wording.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::PrefixNotOneSegment { .. } => "ADDR_001",
            Self::DuplicatePrefix { .. } => "ADDR_002",
            Self::SlugNotOneSegment { .. } => "ADDR_003",
            Self::SlugCollision { .. } => "ADDR_004",
            Self::SlugOnRootPage { .. } => "ADDR_005",
            Self::SlugShadowsSibling { .. } => "ADDR_006",
            Self::UnprefixedLocaleIsPrefixed { .. } => "ADDR_007",
            Self::OriginInvalid { .. } => "ADDR_008",
            Self::OriginNotHttps { .. } => "ADDR_009",
            Self::DefaultLanguageConflicts { .. } => "ADDR_010",
            Self::LanguageOriginsInvalid { .. } => "ADDR_011",
            Self::SlugOnCapture { .. } => "ADDR_012",
        }
    }
}

impl fmt::Display for AddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrefixNotOneSegment { prefix } => write!(
                f,
                "'{prefix}' is not one path segment — a locale prefix is a single leading segment, with no '/' and not empty"
            ),
            Self::DuplicatePrefix { prefix } => {
                write!(f, "the prefix '{prefix}' is claimed twice")
            }
            Self::SlugNotOneSegment { page, slug } => write!(
                f,
                "'{page}' declares the slug '{slug}', which is not one path segment — a slug carries no '/', '?' or '#', and is not empty"
            ),
            Self::SlugCollision {
                parent,
                language,
                slug,
            } => write!(
                f,
                "two pages under '{parent}' both claim the slug '{slug}' in '{language}' — one address cannot serve two pages"
            ),
            Self::SlugOnRootPage { language, slug } => write!(
                f,
                "the root page declares the slug '{slug}' ('{language}'), but '/' has no segment of its own to translate — did you mean a directory's index page?"
            ),
            Self::SlugShadowsSibling {
                parent,
                language,
                slug,
                shadowed,
            } => write!(
                f,
                "the slug '{slug}' ('{language}', under '{parent}') is the canonical stem of '{shadowed}', which would then be unreachable in that language"
            ),
            Self::UnprefixedLocaleIsPrefixed { locale, prefix } => write!(
                f,
                "'{locale}' is declared the language of unprefixed pages and is also served under '/{prefix}' — one language has one address per page"
            ),
            Self::OriginInvalid { origin, reason } => {
                write!(f, "the origin '{origin}' {reason}")
            }
            Self::OriginNotHttps { origin } => write!(
                f,
                "the origin '{origin}' is not https — a published page is named by an https URL"
            ),
            Self::DefaultLanguageConflicts { locale, with } => write!(
                f,
                "'{locale}' is declared the default language, which replaces {with} — declare one or the other"
            ),
            Self::LanguageOriginsInvalid { reason, value } => {
                write!(f, "'{value}': {reason}")
            }
            Self::SlugOnCapture {
                page,
                language,
                slug,
            } => write!(
                f,
                "'{page}' is a capture: its value is the caller's and has no slug in '{language}' ('{slug}') — declare it on a page beneath it"
            ),
        }
    }
}

impl core::error::Error for AddressError {}

impl crate::Error for AddressError {
    fn error_code(&self) -> &'static str {
        self.code()
    }

    /// The whole `Display` line: only an operator at boot reads it, so it names both sides.
    fn display_message(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(feature = "std"))]
    use alloc::string::ToString;

    #[test]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn every_variant_has_its_own_code() {
        let codes = [
            AddressError::PrefixNotOneSegment {
                prefix: String::new(),
            }
            .code(),
            AddressError::DuplicatePrefix {
                prefix: String::new(),
            }
            .code(),
            AddressError::SlugNotOneSegment {
                page: String::new(),
                slug: String::new(),
            }
            .code(),
            AddressError::SlugCollision {
                parent: String::new(),
                language: String::new(),
                slug: String::new(),
            }
            .code(),
            AddressError::SlugOnRootPage {
                language: String::new(),
                slug: String::new(),
            }
            .code(),
            AddressError::SlugShadowsSibling {
                parent: String::new(),
                language: String::new(),
                slug: String::new(),
                shadowed: String::new(),
            }
            .code(),
            AddressError::UnprefixedLocaleIsPrefixed {
                locale: String::new(),
                prefix: String::new(),
            }
            .code(),
            AddressError::OriginInvalid {
                origin: String::new(),
                reason: "",
            }
            .code(),
            AddressError::OriginNotHttps {
                origin: String::new(),
            }
            .code(),
            AddressError::DefaultLanguageConflicts {
                locale: String::new(),
                with: "",
            }
            .code(),
            AddressError::LanguageOriginsInvalid {
                reason: "",
                value: String::new(),
            }
            .code(),
            AddressError::SlugOnCapture {
                page: String::new(),
                language: String::new(),
                slug: String::new(),
            }
            .code(),
        ];
        let mut seen = Vec::from(codes);
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), codes.len(), "codes must be distinct: {codes:?}");
    }

    /// `AnyError::new` takes this crate's `Error`, so `page_slugs()?` needs the impl.
    #[test]
    fn it_travels_as_an_any_error_without_losing_its_code_or_its_names() {
        let error = AddressError::SlugCollision {
            parent: "/guides".to_string(),
            language: "nl".to_string(),
            slug: "gidsen".to_string(),
        };
        let code = crate::Error::error_code(&error);
        let message = crate::Error::display_message(&error).into_owned();

        let any = crate::AnyError::new(error);
        assert_eq!(any.get().error_code(), code);
        assert_eq!(any.get().error_code(), "ADDR_004");
        assert!(
            message.contains("/guides") && message.contains("gidsen") && message.contains("nl"),
            "the message an operator sees must name the collision: {message}"
        );
    }

    /// An operator at boot has only this line to work from.
    #[test]
    fn a_shadowing_message_names_both_pages() {
        let message = AddressError::SlugShadowsSibling {
            parent: "/".to_string(),
            language: "nl".to_string(),
            slug: "trust".to_string(),
            shadowed: "/trust".to_string(),
        }
        .to_string();
        assert!(message.contains("trust"), "{message}");
        assert!(message.contains("/trust"), "{message}");
        assert!(message.contains("nl"), "{message}");
    }
}
