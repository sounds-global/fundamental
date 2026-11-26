use std::collections::BTreeSet;
use std::fmt::Debug;

use fundamental_core::Variant;
use fundamental_core::validate::ValidationErrors;
use fundamental_db::ident::is_stored_name;

use crate::JobError;

/// The closed set of jobs an application schedules — `hold.lapse`, `bouwcheck.due`,
/// `invoice.reminder` — declared as the application's own enum with `#[derive(Variant)]`.
///
/// A kind's value is its stored identity: renaming one orphans every pending job of the old name.
pub trait JobKind: Variant + Eq + Send + Sync + Debug {}

/// Checks `K`'s declared kinds: an empty set, a value out of shape, a value declared twice.
pub(crate) fn validate_kinds<K: JobKind>() -> Result<(), JobError> {
    let mut problems = ValidationErrors::new();
    if K::OPTIONS.is_empty() {
        problems.add_for_field(
            "kinds",
            "empty",
            "an application declares at least one kind",
        );
    }
    let mut seen = BTreeSet::new();
    for kind in K::OPTIONS {
        let name = kind.value();
        if !is_stored_name(name) {
            problems.add_for_field(
                "kinds",
                "pattern",
                format!("`{name}` is not [a-z][a-z0-9._-]{{0,63}}"),
            );
        }
        if !seen.insert(name) {
            problems.add_for_field("kinds", "duplicate", format!("`{name}` is declared twice"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(JobError::InvalidKinds { problems })
    }
}

/// Checks that `K` declares no kind value another served set already declares.
pub(crate) fn validate_disjoint<K: JobKind>(served: &[&'static str]) -> Result<(), JobError> {
    let mut problems = ValidationErrors::new();
    for kind in K::OPTIONS {
        let name = kind.value();
        if served.contains(&name) {
            problems.add_for_field(
                "kinds",
                "served_twice",
                format!("`{name}` is already served by another set on this surface"),
            );
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(JobError::InvalidKinds { problems })
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;
    use fundamental_macros::Variant;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
    enum Deadline {
        #[variant(value = "hold.lapse")]
        HoldLapse,
        #[variant(value = "invoice.reminder")]
        Reminder,
    }

    impl JobKind for Deadline {}

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
    enum Broken {
        #[variant(value = "Hold Lapse")]
        Shouting,
        #[variant(value = "reminder")]
        Twice,
        #[variant(value = "reminder")]
        Again,
    }

    impl JobKind for Broken {}

    /// Declared by hand, so it can be empty, which the derive refuses.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Nothing {}

    impl Variant for Nothing {
        const KEY: &'static str = "nothing";
        const OPTIONS: &'static [Self] = &[];

        fn value(&self) -> &'static str {
            match *self {}
        }

        fn from_value(_value: &str) -> Option<Self> {
            None
        }
    }

    impl JobKind for Nothing {}

    #[test]
    fn a_declared_kind_parses_back_from_its_stored_name() {
        assert_eq!(
            Deadline::from_value("hold.lapse"),
            Some(Deadline::HoldLapse)
        );
        assert_eq!(
            Deadline::from_value("invoice.reminder"),
            Some(Deadline::Reminder)
        );
        assert_eq!(Deadline::from_value("hold.expire"), None);
        assert!(validate_kinds::<Deadline>().is_ok());
    }

    #[test]
    fn every_problem_with_the_kinds_is_reported_together() {
        let error = validate_kinds::<Broken>().expect_err("refused");
        assert_eq!(error.error_code(), "JOBS_002");
        let JobError::InvalidKinds { problems } = error else {
            panic!("not an InvalidKinds: {error:?}");
        };
        let codes: Vec<_> = problems
            .errors_for_field("kinds")
            .iter()
            .map(|problem| problem.code.as_str())
            .collect();
        assert_eq!(codes, ["pattern", "duplicate"]);
    }

    #[test]
    fn an_application_with_no_kinds_is_refused() {
        let error = validate_kinds::<Nothing>().expect_err("refused");
        assert_eq!(error.error_code(), "JOBS_002");
    }
}
