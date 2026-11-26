//! [`LedgerError`]: what the migration ledger refuses.

use fundamental_macros::Error;

use crate::migration::generate::SchemaDifference;

/// What the migration ledger refuses: an `apply` past a failed migration, and every
/// `migrate resolve` that cannot be proved or recorded.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LedgerError {
    /// A migration failed and nothing has settled it, so nothing after it is applied.
    #[error(
        code = "MIGRATE_016",
        message = "{name} failed at step {step} of {total} and is unresolved; see `migrate \
                   status`, then `migrate resolve {name} --retry | --applied --reason <text>`",
        status = 409
    )]
    Unresolved {
        /// The migration's ledger name.
        name: String,
        /// The step it failed at.
        step: usize,
        /// How many steps it has.
        total: usize,
    },

    /// `resolve` named a migration with no failed step.
    #[error(
        code = "MIGRATE_017",
        message = "{name} has no failed step to resolve; `migrate status` shows where each \
                   migration stands",
        status = 409
    )]
    NothingToResolve {
        /// The name given.
        name: String,
    },

    /// `resolve --retry` of a step whose answer was lost and whose statement cannot run again.
    #[error(
        code = "MIGRATE_018",
        message = "{name} cannot be retried: step {step} of {total} is {why}, and its answer was \
                   lost, so whether it applied is unknown; once the catalog shows the \
                   migration, settle it with --applied",
        status = 409
    )]
    NotRerunnable {
        /// The migration's ledger name.
        name: String,
        /// The step that cannot run again.
        step: usize,
        /// How many steps it has.
        total: usize,
        /// Why it cannot run again.
        why: String,
    },

    /// `resolve --applied` of a migration whose effect the live catalog does not show.
    #[error(
        code = "MIGRATE_019",
        message = "{name} is not in the live catalog: {summary}",
        status = 409
    )]
    NotInCatalog {
        /// The migration's ledger name.
        name: String,
        /// Each effect the live catalog lacks or holds otherwise.
        missing: Vec<SchemaDifference>,
        /// The same, as a person reads it.
        summary: String,
    },

    /// `resolve --applied` of a migration the catalog cannot show: its statements write rows,
    /// or make an object the catalog snapshot does not hold.
    #[error(
        code = "MIGRATE_020",
        message = "{name} changes nothing the catalog shows, so --applied cannot be proved; \
                   settle it with --retry",
        status = 409
    )]
    NoCatalogEffect {
        /// The migration's ledger name.
        name: String,
    },

    /// Nobody to record: neither `SUDO_USER` nor `USER` is set.
    #[error(
        code = "MIGRATE_021",
        message = "who resolves is read from SUDO_USER or USER, and neither is set",
        status = 400
    )]
    NoOperator,

    /// No reason to record.
    #[error(
        code = "MIGRATE_022",
        message = "a resolution is recorded with why: give --reason <text>",
        status = 400
    )]
    NoReason,

    /// `resolve --applied` of a migration with a step, from the failed one on, whose effect the
    /// catalog cannot show: rows it writes, or an object the catalog snapshot does not hold.
    #[error(
        code = "MIGRATE_023",
        message = "{name} cannot be proved applied: step {step} of {total} ({statement}) \
                   changes nothing the catalog shows, so whether it ran is unknown; settle it \
                   with --retry",
        status = 409
    )]
    StepNotInCatalog {
        /// The migration's ledger name.
        name: String,
        /// The step the catalog cannot show.
        step: usize,
        /// How many steps it has.
        total: usize,
        /// Its statement.
        statement: String,
    },

    /// `resolve --retry` of a migration whose remaining steps build an index the live catalog
    /// holds unfinished: `CREATE INDEX IF NOT EXISTS` would pass over it and leave it unusable.
    #[error(
        code = "MIGRATE_024",
        message = "{name} cannot be retried: index {index} on {table} is in the catalog but its \
                   build did not finish, and step {step} would pass over it; drop the index, \
                   then retry",
        status = 409
    )]
    UnfinishedIndex {
        /// The migration's ledger name.
        name: String,
        /// The step that builds it.
        step: usize,
        /// The index.
        index: String,
        /// Its table.
        table: String,
    },
}

impl LedgerError {
    /// [`NotInCatalog`](Self::NotInCatalog), its summary written from `missing`.
    #[must_use]
    pub fn not_in_catalog(name: &str, missing: Vec<SchemaDifference>) -> Self {
        let summary = missing
            .iter()
            .map(
                |difference| match (&difference.expected, &difference.found) {
                    (Some(_), None) => {
                        format!("{} on {} is missing", difference.item, difference.table)
                    }
                    (None, Some(_)) => {
                        format!("{} on {} is still there", difference.item, difference.table)
                    }
                    (expected, found) => format!(
                        "{} on {} is {}, not {}",
                        difference.item,
                        difference.table,
                        found.as_deref().unwrap_or("none"),
                        expected.as_deref().unwrap_or("none"),
                    ),
                },
            )
            .collect::<Vec<_>>()
            .join("; ");
        Self::NotInCatalog {
            name: name.to_owned(),
            missing,
            summary,
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::LedgerError;
    use crate::migration::generate::{SchemaDifference, SchemaItem};

    #[test]
    fn each_refusal_has_its_own_code() {
        let codes: Vec<&str> = [
            LedgerError::Unresolved {
                name: "a".to_owned(),
                step: 2,
                total: 3,
            },
            LedgerError::NothingToResolve {
                name: "a".to_owned(),
            },
            LedgerError::NotRerunnable {
                name: "a".to_owned(),
                step: 2,
                total: 3,
                why: "a RENAME".to_owned(),
            },
            LedgerError::not_in_catalog("a", Vec::new()),
            LedgerError::NoCatalogEffect {
                name: "a".to_owned(),
            },
            LedgerError::NoOperator,
            LedgerError::NoReason,
            LedgerError::StepNotInCatalog {
                name: "a".to_owned(),
                step: 2,
                total: 3,
                statement: "UPDATE a SET b = 1".to_owned(),
            },
            LedgerError::UnfinishedIndex {
                name: "a".to_owned(),
                step: 2,
                index: "b".to_owned(),
                table: "c".to_owned(),
            },
        ]
        .iter()
        .map(|error| error.error_code())
        .collect();
        assert_eq!(
            codes,
            [
                "MIGRATE_016",
                "MIGRATE_017",
                "MIGRATE_018",
                "MIGRATE_019",
                "MIGRATE_020",
                "MIGRATE_021",
                "MIGRATE_022",
                "MIGRATE_023",
                "MIGRATE_024"
            ]
        );
    }

    #[test]
    fn an_unresolved_migration_names_where_to_look_and_how_to_settle_it() {
        let message = LedgerError::Unresolved {
            name: "0007_index_orders".to_owned(),
            step: 2,
            total: 3,
        }
        .to_string();
        assert!(
            message.starts_with("0007_index_orders failed at step 2 of 3 and is unresolved"),
            "{message}"
        );
        assert!(message.contains("migrate status"), "{message}");
        assert!(
            message.contains("migrate resolve 0007_index_orders"),
            "{message}"
        );
    }

    #[test]
    fn what_the_catalog_lacks_is_named_item_by_item() {
        let message = LedgerError::not_in_catalog(
            "0007_index_orders",
            vec![SchemaDifference {
                table: "orders".to_owned(),
                item: SchemaItem::Index("orders_by_customer".to_owned()),
                expected: Some("(customer_id HASH)".to_owned()),
                found: None,
            }],
        )
        .to_string();
        assert_eq!(
            message,
            "0007_index_orders is not in the live catalog: index orders_by_customer on orders \
             is missing"
        );
    }
}
