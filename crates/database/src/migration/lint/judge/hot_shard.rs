//! `hot_shard`: a key that leads with an ever-increasing value split in order.

use super::Before;
use crate::migration::ddl::{DdlStatement, KeyDeclaration, Sharding};
use crate::migration::lint::{Lint, LintFinding, Lints};

/// Fires on a key led `ASC` or `DESC` by a column that only increases, so every new row lands
/// on the last tablet. A table sharing the colocated tablet is not judged.
pub(crate) fn judge(
    before: &Before<'_>,
    lints: &Lints,
    statement: &DdlStatement,
) -> Option<LintFinding> {
    if shares_the_tablet(before, lints, statement) {
        return None;
    }
    let (verb, object, key, column) = match statement {
        DdlStatement::CreateTable {
            table,
            primary_key: Some(key),
            columns,
            ..
        } => {
            let lead = leading(key)?;
            (
                "CREATE TABLE",
                table,
                key,
                columns.iter().find(|column| column.name == lead)?,
            )
        }
        DdlStatement::CreateIndex {
            name, table, key, ..
        } => {
            let lead = leading(key)?;
            (
                "CREATE INDEX",
                name,
                key,
                before.facts.tables.get(table)?.column(lead)?,
            )
        }
        _ => return None,
    };
    let split = key.elements().first()?.sharding()?;
    column.only_increases().then(|| {
        before
            .finding(
                Lint::HotShard,
                statement,
                verb,
                format!(
                    "{object} leads its key with {} {}, a value that only increases, so every new \
                 row lands on one tablet",
                    column.name,
                    split_word(split)
                ),
            )
            .naming(object.clone())
    })
}

fn shares_the_tablet(before: &Before<'_>, lints: &Lints, statement: &DdlStatement) -> bool {
    if !lints.is_colocated() {
        return false;
    }
    let colocated = match statement {
        DdlStatement::CreateTable { colocated, .. } => *colocated,
        DdlStatement::CreateIndex { table, .. } => before
            .facts
            .tables
            .get(table)
            .and_then(|facts| facts.colocated),
        _ => None,
    };
    colocated != Some(false)
}

fn leading(key: &KeyDeclaration) -> Option<&str> {
    let first = key.elements().first()?;
    match (first.columns(), first.sharding()) {
        ([column], Some(Sharding::Asc | Sharding::Desc)) => Some(column.as_str()),
        _ => None,
    }
}

fn split_word(split: Sharding) -> &'static str {
    match split {
        Sharding::Hash => "HASH",
        Sharding::Asc => "ASC",
        Sharding::Desc => "DESC",
    }
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::judge::test_support::{ORDERS, fired, fired_under};
    use crate::migration::lint::{Lint, Lints};

    #[test]
    fn a_table_or_an_index_led_by_a_time_in_order_fires() {
        let table = [(
            "0001_events",
            "CREATE TABLE events (at TIMESTAMPTZ NOT NULL, id UUID NOT NULL, \
             PRIMARY KEY (at DESC, id ASC));",
        )];
        let index = [
            ORDERS,
            (
                "0002_by_at",
                "CREATE INDEX orders_by_at ON orders (at ASC);",
            ),
        ];

        let by_table = fired(Lint::HotShard, &table);
        let by_index = fired(Lint::HotShard, &index);

        assert_eq!(by_table.len(), 1);
        assert!(
            by_table[0].at.contains("leads its key with at DESC"),
            "{}",
            by_table[0].at
        );
        assert_eq!(by_index.len(), 1);
        assert_eq!(
            (by_index[0].table.as_str(), by_index[0].object.as_str()),
            ("orders", "orders_by_at")
        );
    }

    #[test]
    fn a_hashed_lead_or_a_time_after_it_does_not_fire() {
        let led_by_hash = [
            ORDERS,
            (
                "0002_by_at",
                "CREATE INDEX orders_by_at ON orders (id HASH, at DESC);",
            ),
        ];
        let hashed_time = [
            ORDERS,
            (
                "0002_by_at",
                "CREATE INDEX orders_by_at ON orders (at HASH);",
            ),
        ];
        let ordered_text = [
            ORDERS,
            (
                "0002_by_note",
                "CREATE INDEX orders_by_note ON orders (note ASC);",
            ),
        ];

        for files in [&led_by_hash, &hashed_time, &ordered_text] {
            assert!(fired(Lint::HotShard, files).is_empty());
        }
    }

    #[test]
    fn a_table_that_shares_the_colocated_tablet_does_not_fire() {
        let colocated = Lints::default().colocated(true);
        let together = [(
            "0001_events",
            "CREATE TABLE events (at TIMESTAMPTZ NOT NULL, PRIMARY KEY (at DESC));",
        )];
        let apart = [(
            "0001_events",
            "CREATE TABLE events (at TIMESTAMPTZ NOT NULL, PRIMARY KEY (at DESC)) \
             WITH (COLOCATION = false);",
        )];

        assert!(fired_under(&colocated, Lint::HotShard, &together).is_empty());
        assert_eq!(fired_under(&colocated, Lint::HotShard, &apart).len(), 1);
    }
}
