//! [`MigrationCheck`] — what `fun migrate check` found, and how much of it to believe.

use fundamental_db::migration::SchemaDrift;

/// The result of checking a database against the migrations and models on disk.
///
/// Unapplied files are exact and refuse the check; drift is advisory and never fails it.
#[derive(Debug, Clone, Default)]
pub struct MigrationCheck {
    /// Migration files on disk with no record in the ledger, in application order.
    pub unapplied: Vec<String>,
    /// Differences between the live schema and the models. Advisory — see the type docs.
    pub drift: Vec<SchemaDrift>,
    /// What the drift comparison cannot see.
    pub blind_spots: &'static [&'static str],
}

/// What the drift half cannot see, stated wherever it is reported.
pub const DRIFT_BLIND_SPOTS: &[&str] = &[
    "foreign keys: a model does not declare them yet",
    "writers: only a role the model names that the table does not let write is drift, since \
     default privileges grant a deployment's own login every table",
    "functions and triggers: no table's shape holds them; their files are hashed in the ledger",
];

impl MigrationCheck {
    /// Whether anything on disk has not been applied.
    #[must_use]
    pub fn has_unapplied(&self) -> bool {
        !self.unapplied.is_empty()
    }

    /// A human-readable summary, including what the drift half could not examine.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = if self.unapplied.is_empty() {
            "every migration on disk is applied".to_owned()
        } else {
            format!(
                "{} migration(s) on disk are not applied: {}",
                self.unapplied.len(),
                self.unapplied.join(", ")
            )
        };
        if self.drift.is_empty() {
            out.push_str("; the models and the live schema agree as far as this can tell");
        } else {
            out.push_str(&format!(
                "; {} advisory drift finding(s) — not a failure, and not a verified schema",
                self.drift.len()
            ));
        }
        out.push_str("\nthis check cannot see: ");
        out.push_str(&self.blind_spots.join("; "));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{DRIFT_BLIND_SPOTS, MigrationCheck};

    #[test]
    fn a_clean_check_still_says_what_it_could_not_examine() {
        let check = MigrationCheck {
            blind_spots: DRIFT_BLIND_SPOTS,
            ..MigrationCheck::default()
        };

        assert!(!check.has_unapplied());
        let summary = check.summary();
        assert!(summary.contains("every migration on disk is applied"));
        assert!(
            summary.contains("foreign keys"),
            "a green check that implied a verified schema would be worse than none: {summary}"
        );
    }

    #[test]
    fn unapplied_migrations_are_named() {
        let check = MigrationCheck {
            unapplied: vec!["0007_add_widgets".to_owned()],
            blind_spots: DRIFT_BLIND_SPOTS,
            ..MigrationCheck::default()
        };

        assert!(check.has_unapplied());
        assert!(check.summary().contains("0007_add_widgets"));
    }
}
