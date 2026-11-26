//! [`LintFinding`]: one lint that fired, where, and what to do.

use std::fmt;

use fundamental_core::model::sql::Lint;

/// One lint that fired: on which table, in which file, at which statement, and why.
///
/// Its message names the lint and says its fix.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LintFinding {
    /// The lint.
    pub lint: Lint,
    /// The table it fired on: the table whose model accepts it.
    pub table: String,
    /// The migration file it fired in; none for a change no file holds yet.
    pub migration: Option<String>,
    /// The statement's verb, as the recogniser names it; empty for the table as a whole.
    pub verb: String,
    /// What it names: a table, an index, or `<table>.<column>`.
    pub object: String,
    /// What was found.
    pub at: String,
}

impl LintFinding {
    /// A finding of `lint` on `table`, naming `object`, found to be `at`.
    #[must_use]
    pub fn new(
        lint: Lint,
        table: impl Into<String>,
        object: impl Into<String>,
        at: impl Into<String>,
    ) -> Self {
        Self {
            lint,
            table: table.into(),
            migration: None,
            verb: String::new(),
            object: object.into(),
            at: at.into(),
        }
    }

    /// It, found in the file `migration`.
    #[must_use]
    pub fn in_file(mut self, migration: impl Into<String>) -> Self {
        self.migration = Some(migration.into());
        self
    }

    /// It, naming `object`.
    #[must_use]
    pub fn naming(mut self, object: impl Into<String>) -> Self {
        self.object = object.into();
        self
    }

    /// It, found at a statement of `verb`.
    #[must_use]
    pub fn by(mut self, verb: impl Into<String>) -> Self {
        self.verb = verb.into();
        self
    }
}

impl fmt::Display for LintFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}  ", self.lint.code(), self.lint.name())?;
        if let Some(migration) = &self.migration {
            write!(f, "{migration}: ")?;
        }
        write!(
            f,
            "{}. Fix: {}, or accept it on the model of {}: \
             `#[model(accept({} = \"why it is right here\"))]`.",
            self.at,
            self.lint.fix(),
            self.table,
            self.lint.name()
        )
    }
}

pub(crate) fn report(findings: &[LintFinding]) -> String {
    findings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use fundamental_core::model::sql::Lint;

    use super::LintFinding;

    #[test]
    fn a_finding_says_its_code_its_file_what_it_found_and_the_fix() {
        let finding = LintFinding::new(
            Lint::UnindexedReference,
            "import_events",
            "import_events.project_id",
            "import_events.project_id refers to projects and leads no index",
        )
        .in_file("acme/0004_import_events");

        let said = finding.to_string();

        assert!(said.starts_with("MIG_L07 unindexed_reference  acme/0004_import_events: "));
        assert!(said.contains("refers to projects and leads no index"));
        assert!(said.contains(Lint::UnindexedReference.fix()));
        assert!(said.contains("#[model(accept(unindexed_reference = "));
    }
}
