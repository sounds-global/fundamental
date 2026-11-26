//! [`Exemption`]: a key site the guard passes, and the reason it does.

use fundamental_core::model::sql::Lint;

/// One object the guard does not judge, with the reason inline.
///
/// The reason is required, so a reviewer can tell whether it is still true.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exemption {
    /// The table or index name this exempts.
    pub object: &'static str,
    /// Why it is exempt; never a placeholder.
    pub reason: &'static str,
    /// The verb this passes on `object`, such as `DROP INDEX`; `None` for a key site.
    pub dropped: Option<&'static str>,
    /// The lint this accepts on the table `object` in the app's own files; `None` otherwise.
    pub accepted: Option<Lint>,
}

impl Exemption {
    /// Declares `object` exempt, `because` of a stated reason.
    #[must_use]
    pub const fn new(object: &'static str, because: &'static str) -> Self {
        Self {
            object,
            reason: because,
            dropped: None,
            accepted: None,
        }
    }

    /// Accepts `lint` on the table `object` in the app's own files, `because` of a stated
    /// reason.
    #[must_use]
    pub const fn accept(lint: Lint, object: &'static str, because: &'static str) -> Self {
        Self {
            object,
            reason: because,
            dropped: None,
            accepted: Some(lint),
        }
    }

    /// The exemption accepting `lint` on the table `object`, if one is declared.
    #[must_use]
    pub fn find_accept<'a>(exemptions: &'a [Self], lint: Lint, object: &str) -> Option<&'a Self> {
        exemptions
            .iter()
            .find(|e| e.accepted == Some(lint) && e.object == object)
    }

    /// Declares that one destructive statement, `verb` on `object`, is passed while the
    /// `destructive` lint warns, `because` of a stated reason.
    ///
    /// A table or index is named alone (`"old_unique"`); a column, constraint or policy with its
    /// table (`"widgets.legacy"`); a rename by the name before.
    #[must_use]
    pub const fn drop_of(verb: &'static str, object: &'static str, because: &'static str) -> Self {
        Self {
            object,
            reason: because,
            dropped: Some(verb),
            accepted: None,
        }
    }

    /// Whether `object` appears in `exemptions`, and why.
    #[must_use]
    pub fn find<'a>(exemptions: &'a [Self], object: &str) -> Option<&'a Self> {
        exemptions
            .iter()
            .find(|e| e.dropped.is_none() && e.accepted.is_none() && e.object == object)
    }

    /// The exemption passing `verb` on `object`, if one is declared.
    #[must_use]
    pub fn find_drop<'a>(exemptions: &'a [Self], verb: &str, object: &str) -> Option<&'a Self> {
        exemptions
            .iter()
            .find(|e| e.dropped == Some(verb) && e.object == object)
    }
}

#[cfg(test)]
mod tests {
    use super::Exemption;

    const EXEMPT: &[Exemption] = &[Exemption::new(
        "_fundamental_migrations",
        "the ledger itself, created by the runner before any migration is judged",
    )];

    #[test]
    fn an_exempt_object_is_found_with_its_reason() {
        let found = Exemption::find(EXEMPT, "_fundamental_migrations").expect("exempt");

        assert!(found.reason.contains("the ledger itself"));
    }

    #[test]
    fn anything_else_is_not_exempt() {
        assert!(Exemption::find(EXEMPT, "permission_grants").is_none());
    }
}
