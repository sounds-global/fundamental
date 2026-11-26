//! [`CatalogProof`]: that the live catalog shows every effect of one migration.

use std::collections::BTreeSet;

use super::{LedgerError, StepReplay};
use crate::migration::generate::{SchemaDifference, SchemaItem, Snapshot};

/// That the live catalog shows every effect one migration has, so recording it as applied
/// states a fact rather than someone's word.
///
/// The effect is what differs between the catalog before the migration and after its steps
/// ([`StepReplay`]); each must be in the live catalog, and an unfinished index counts as
/// missing. Every step from the failed one on must change the catalog or leave nothing
/// behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogProof {
    effects: usize,
}

impl CatalogProof {
    /// Proves `name`'s effect, as `replay` has it, is in `live`, from step `failed` on;
    /// `unusable` names each `(table, index)` whose build did not finish.
    ///
    /// # Errors
    /// [`LedgerError::NoCatalogEffect`] when the replay changes no catalog;
    /// [`LedgerError::StepNotInCatalog`] for the first step from `failed` on the catalog cannot
    /// show; [`LedgerError::NotInCatalog`] naming every effect `live` lacks or holds otherwise.
    pub fn check(
        name: &str,
        replay: &StepReplay,
        failed: usize,
        live: &Snapshot,
        unusable: &[(String, String)],
    ) -> Result<Self, LedgerError> {
        let after = replay.after();
        let effects = replay.before().differences(after);
        if effects.is_empty() {
            return Err(LedgerError::NoCatalogEffect {
                name: name.to_owned(),
            });
        }
        every_step_shown(name, replay, failed)?;
        let made: BTreeSet<&str> = effects
            .iter()
            .filter(|effect| effect.item == SchemaItem::Table && effect.found.is_some())
            .map(|effect| effect.table.as_str())
            .collect();
        let is_effect = |difference: &SchemaDifference| {
            effects
                .iter()
                .any(|effect| effect.table == difference.table && effect.item == difference.item)
                || (made.contains(difference.table.as_str()) && difference.expected.is_some())
        };
        let mut missing: Vec<SchemaDifference> = after
            .differences(live)
            .into_iter()
            .filter(is_effect)
            .collect();
        for (table, index) in unusable {
            let item = SchemaItem::Index(index.clone());
            let wanted = SchemaDifference {
                table: table.clone(),
                item: item.clone(),
                expected: Some(String::new()),
                found: None,
            };
            let listed = missing
                .iter()
                .any(|difference| difference.table == *table && difference.item == item);
            if is_effect(&wanted) && !listed {
                missing.push(SchemaDifference {
                    table: table.clone(),
                    item,
                    expected: Some("usable".to_owned()),
                    found: Some("unusable: its build did not finish".to_owned()),
                });
            }
        }
        if missing.is_empty() {
            return Ok(Self {
                effects: effects.len(),
            });
        }
        Err(LedgerError::not_in_catalog(name, missing))
    }

    /// How many effects were found in the live catalog.
    #[must_use]
    pub fn effects(&self) -> usize {
        self.effects
    }
}

/// Refuses the first step from `failed` on that neither changes the catalog nor leaves
/// nothing behind.
fn every_step_shown(name: &str, replay: &StepReplay, failed: usize) -> Result<(), LedgerError> {
    match replay
        .changes()
        .filter(|(step, _, _)| *step >= failed)
        .find(|(_, statement, changed)| !changed && !leaves_nothing(statement))
    {
        Some((step, statement, _)) => Err(LedgerError::StepNotInCatalog {
            name: name.to_owned(),
            step,
            total: replay.total(),
            statement: statement.trim().to_owned(),
        }),
        None => Ok(()),
    }
}

/// Whether `statement` leaves nothing behind once it has run.
fn leaves_nothing(statement: &str) -> bool {
    statement.split_whitespace().next().is_some_and(|word| {
        [
            "SELECT", "ANALYZE", "VACUUM", "SET", "RESET", "SHOW", "EXPLAIN",
        ]
        .iter()
        .any(|verb| word.trim_end_matches(';').eq_ignore_ascii_case(verb))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use fundamental_core::model::sql::ShardKind;

    use super::CatalogProof;
    use crate::migration::StepReplay;
    use crate::migration::generate::{
        SchemaItem, Snapshot, SnapshotColumn, SnapshotIndex, SnapshotTable,
    };
    use crate::migration::{KeyPart, LedgerError};

    fn orders(with_index: bool) -> Snapshot {
        let column = |name: &str| SnapshotColumn {
            name: name.to_owned(),
            data_type: "UUID".to_owned(),
            nullable: false,
            default: None,
        };
        let mut table = SnapshotTable {
            columns: vec![column("id"), column("customer_id")],
            key: KeyPart::parts(&[("id", Some(ShardKind::Hash))]),
            ..SnapshotTable::default()
        };
        if with_index {
            table.indexes.insert(
                "orders_by_customer".to_owned(),
                SnapshotIndex {
                    key: KeyPart::parts(&[("customer_id", Some(ShardKind::Hash))]),
                    unique: false,
                    condition: None,
                    include: Vec::new(),
                    constraint: false,
                },
            );
        }
        Snapshot {
            tables: BTreeMap::from([("orders".to_owned(), table)]),
            ..Snapshot::default()
        }
    }

    fn replay(before: Snapshot, after: Snapshot) -> StepReplay {
        let mut replay = StepReplay::new(before);
        replay.step(
            "CREATE INDEX IF NOT EXISTS orders_by_customer ON orders (customer_id HASH)",
            after,
        );
        replay
    }

    #[test]
    fn a_migration_whose_index_is_live_is_proved() {
        let proof = CatalogProof::check(
            "0007_index_orders",
            &replay(orders(false), orders(true)),
            1,
            &orders(true),
            &[],
        )
        .expect("the index is live");
        assert_eq!(proof.effects(), 1);
    }

    #[test]
    fn a_missing_index_is_named() {
        let refused = CatalogProof::check(
            "0007_index_orders",
            &replay(orders(false), orders(true)),
            1,
            &orders(false),
            &[],
        )
        .expect_err("the index is not live");
        let LedgerError::NotInCatalog { missing, .. } = &refused else {
            panic!("expected NotInCatalog, got {refused:?}");
        };
        assert_eq!(missing.len(), 1);
        assert_eq!(
            missing[0].item,
            SchemaItem::Index("orders_by_customer".to_owned())
        );
        assert!(
            refused
                .to_string()
                .ends_with("index orders_by_customer on orders is missing"),
            "{refused}"
        );
    }

    #[test]
    fn an_index_whose_build_did_not_finish_counts_as_missing() {
        let refused = CatalogProof::check(
            "0007_index_orders",
            &replay(orders(false), orders(true)),
            1,
            &orders(true),
            &[("orders".to_owned(), "orders_by_customer".to_owned())],
        )
        .expect_err("the index is unusable");
        assert!(
            refused
                .to_string()
                .contains("index orders_by_customer on orders is unusable"),
            "{refused}"
        );
    }

    #[test]
    fn a_difference_the_migration_did_not_make_is_not_its_to_prove() {
        let mut live = orders(true);
        live.tables
            .insert("drafts".to_owned(), SnapshotTable::default());
        CatalogProof::check(
            "0007_index_orders",
            &replay(orders(false), orders(true)),
            1,
            &live,
            &[],
        )
        .expect("a table some other change made is not this migration's effect");
    }

    #[test]
    fn a_table_the_migration_made_is_proved_whole() {
        let empty = Snapshot::default();
        let mut live = orders(true);
        if let Some(table) = live.tables.get_mut("orders") {
            table.columns.pop();
        }
        let refused =
            CatalogProof::check("0001_orders", &replay(empty, orders(true)), 1, &live, &[])
                .expect_err("a column of the table it made is missing");
        assert!(
            refused.to_string().contains("column customer_id on orders"),
            "{refused}"
        );
    }

    #[test]
    fn a_migration_with_no_catalog_effect_cannot_be_proved() {
        let refused = CatalogProof::check(
            "0009_seed",
            &replay(orders(true), orders(true)),
            1,
            &orders(true),
            &[],
        )
        .expect_err("rows are not in the catalog");
        assert!(matches!(refused, LedgerError::NoCatalogEffect { .. }));
    }

    fn backfill_then_index() -> StepReplay {
        let mut replay = StepReplay::new(Snapshot::default());
        replay.step("CREATE TABLE IF NOT EXISTS orders (…)", orders(false));
        replay.step(
            "UPDATE orders SET customer_id = gen_random_uuid()",
            orders(false),
        );
        replay.step(
            "CREATE INDEX IF NOT EXISTS orders_by_customer ON orders (customer_id HASH)",
            orders(true),
        );
        replay
    }

    #[test]
    fn a_step_from_the_failed_one_on_the_catalog_cannot_show_is_refused() {
        let refused =
            CatalogProof::check("0007_orders", &backfill_then_index(), 2, &orders(true), &[])
                .expect_err("the backfill's rows are not in the catalog");
        let LedgerError::StepNotInCatalog {
            step,
            total,
            statement,
            ..
        } = &refused
        else {
            panic!("expected StepNotInCatalog, got {refused:?}");
        };
        assert_eq!((*step, *total), (2, 3));
        assert!(statement.starts_with("UPDATE orders"), "{statement}");
        assert!(refused.to_string().contains("--retry"), "{refused}");
    }

    #[test]
    fn a_step_before_the_failed_one_ran_and_is_not_asked_again() {
        CatalogProof::check("0007_orders", &backfill_then_index(), 3, &orders(true), &[])
            .expect("the backfill is done, so the index alone is left to prove");
    }

    #[test]
    fn a_step_that_leaves_nothing_behind_needs_no_catalog_to_show_it() {
        let mut replay = StepReplay::new(orders(false));
        replay.step("SELECT pg_sleep(4)", orders(false));
        replay.step("analyze orders;", orders(false));
        replay.step(
            "CREATE INDEX IF NOT EXISTS orders_by_customer ON orders (customer_id HASH)",
            orders(true),
        );
        CatalogProof::check("0007_index_orders", &replay, 1, &orders(true), &[])
            .expect("a sleep and an ANALYZE leave nothing to prove");
    }
}
