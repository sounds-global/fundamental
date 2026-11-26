//! [`SpecialKind`], the classification of a non-page `.rs` file in the pages tree.

use std::path::Path;

use fundamental_core::Variant;
use fundamental_macros::Variant;

use crate::build_error::BuildError;

/// A recognized special (non-page) file name; its value is the file name.
///
/// Each variant's value and the first line of its rustdoc describe one page convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Variant)]
pub(crate) enum SpecialKind {
    /// The HTML shell every page renders inside, at the pages root.
    #[variant(value = "document.rs")]
    Document,
    /// The custom 404 page, at the pages root.
    #[variant(value = "not_found.rs")]
    NotFound,
    /// The page a navigation's error is answered with, at the pages root.
    #[variant(value = "error.rs")]
    Error,
    /// The page a refused request renders, at the pages root.
    #[variant(value = "gate.rs")]
    Gate,
    /// A layout wrapping every page at or below its directory.
    #[variant(value = "layout.rs")]
    Layout,
    /// The server-only loader of the pages in its directory.
    #[variant(value = "loader.rs")]
    Loader,
    /// The policy (who may reach it, caching, title) of every route at or below its directory.
    #[variant(value = "policy.rs")]
    Policy,
}

impl SpecialKind {
    pub(crate) fn classify(name: &str) -> Option<Self> {
        Self::from_value(name)
    }

    /// Whether the file belongs to its directory rather than being recorded once at the root.
    pub(crate) fn is_companion(self) -> bool {
        matches!(self, Self::Layout | Self::Loader | Self::Policy)
    }

    pub(crate) fn label(self) -> &'static str {
        let file = self.value();
        file.strip_suffix(".rs").unwrap_or(file)
    }
}

/// Fail if `name` is a reserved file name (`loading.rs`, `_shell.rs`, `not-found.rs`).
///
/// # Errors
/// Returns [`BuildError::ReservedFile`] naming the path and the remedy.
pub(crate) fn check_reserved(name: &str, rel: &Path) -> Result<(), BuildError> {
    let reason = match name {
        "loading.rs" => Some("reserved for a later loading-fallback port"),
        "_shell.rs" => Some("rename to `document.rs` — `_shell.rs` is an older spelling"),
        "not-found.rs" => {
            Some("rename to `not_found.rs` — `not-found.rs` is not a valid Rust module name")
        }
        _ => None,
    };

    match reason {
        Some(reason) => Err(BuildError::ReservedFile {
            file: rel.to_path_buf(),
            reason: reason.to_string(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn classifies_our_special_names() {
        assert_eq!(
            SpecialKind::classify("document.rs"),
            Some(SpecialKind::Document)
        );
        assert_eq!(
            SpecialKind::classify("not_found.rs"),
            Some(SpecialKind::NotFound)
        );
        assert_eq!(SpecialKind::classify("error.rs"), Some(SpecialKind::Error));
        assert_eq!(
            SpecialKind::classify("layout.rs"),
            Some(SpecialKind::Layout)
        );
        assert_eq!(SpecialKind::classify("gate.rs"), Some(SpecialKind::Gate));
        assert_eq!(
            SpecialKind::classify("loader.rs"),
            Some(SpecialKind::Loader)
        );
        assert_eq!(
            SpecialKind::classify("policy.rs"),
            Some(SpecialKind::Policy)
        );
        assert_eq!(SpecialKind::classify("about.rs"), None);
        assert_eq!(SpecialKind::NotFound.label(), "not_found");
        assert_eq!(SpecialKind::classify("index.rs"), None);
    }

    #[test]
    fn reserved_names_error_with_a_remedy() {
        for name in ["loading.rs", "_shell.rs", "not-found.rs"] {
            let err = check_reserved(name, &PathBuf::from(name)).unwrap_err();
            assert!(matches!(err, BuildError::ReservedFile { .. }), "{name}");
        }
    }

    #[test]
    fn older_spellings_point_at_the_new_names() {
        let shell = check_reserved("_shell.rs", &PathBuf::from("_shell.rs")).unwrap_err();
        assert!(shell.to_string().contains("document.rs"));
        let nf = check_reserved("not-found.rs", &PathBuf::from("not-found.rs")).unwrap_err();
        assert!(nf.to_string().contains("not_found.rs"));
    }

    #[test]
    fn ordinary_names_are_allowed() {
        check_reserved("about.rs", &PathBuf::from("about.rs")).unwrap();
        check_reserved("index.rs", &PathBuf::from("index.rs")).unwrap();
        check_reserved("[id].rs", &PathBuf::from("[id].rs")).unwrap();
        check_reserved("loader.rs", &PathBuf::from("loader.rs")).unwrap();
    }
}
