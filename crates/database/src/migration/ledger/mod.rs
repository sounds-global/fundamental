//! The migration ledger: each migration's steps recorded as they start and end, so a failure
//! says exactly where, and every later `apply` is refused ([`LedgerError::Unresolved`]) until
//! a [`Resolve`] settles it, only by a proven retry or a [`CatalogProof`].

mod catalog_proof;
mod failed_step;
mod ledger_error;
mod ledger_state;
mod migration_ledger;
mod resolution;
mod resolve;
mod resolved;
mod step_record;
mod step_replay;

pub use catalog_proof::CatalogProof;
pub use failed_step::FailedStep;
pub use ledger_error::LedgerError;
pub use ledger_state::LedgerState;
pub use migration_ledger::{LedgerEntry, MigrationLedger};
pub use resolution::Resolution;
pub use resolve::Resolve;
pub use resolved::Resolved;
pub use step_record::{StepRecord, StepState};
pub use step_replay::StepReplay;

/// The table each applied migration is recorded in, once its last step is done.
pub const MIGRATIONS_TABLE: &str = "_fundamental_migrations";

/// The table each step of a migration is recorded in, as it starts and as it ends.
pub const STEPS_TABLE: &str = "_fundamental_migration_steps";

/// The table each `migrate resolve` act is recorded in, with who and why.
pub const RESOLUTIONS_TABLE: &str = "_fundamental_migration_resolutions";

/// Whether `table` is one of the ledger's own, which introspection leaves out and a wipe keeps.
#[must_use]
pub fn is_ledger_table(table: &str) -> bool {
    [MIGRATIONS_TABLE, STEPS_TABLE, RESOLUTIONS_TABLE].contains(&table)
}

#[cfg(test)]
mod tests {
    use super::is_ledger_table;

    #[test]
    fn the_three_ledger_tables_are_the_ledger_s_and_no_other_is() {
        for table in [
            "_fundamental_migrations",
            "_fundamental_migration_steps",
            "_fundamental_migration_resolutions",
        ] {
            assert!(is_ledger_table(table), "{table}");
        }
        for table in ["orders", "_fundamental_migration_lock", ""] {
            assert!(!is_ledger_table(table), "{table}");
        }
    }
}
