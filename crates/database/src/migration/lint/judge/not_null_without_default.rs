//! `not_null_without_default`: `NOT NULL` added where rows may already be, with nothing to fill them.

use super::Before;
use crate::migration::ddl::DdlStatement;
use crate::migration::lint::{Lint, LintFinding};

/// Fires on `ADD COLUMN … NOT NULL` with no `DEFAULT`, and on `SET NOT NULL`, on a table its
/// file did not create.
pub(crate) fn judge(before: &Before<'_>, statement: &DdlStatement) -> Option<LintFinding> {
    match statement {
        DdlStatement::AddColumn {
            table,
            column,
            nullable: false,
            has_default: false,
            ..
        } if !before.created_here.contains(table) => Some(before
            .finding(Lint::NotNullWithoutDefault, statement, "ADD COLUMN", format!(
                "{table}.{column} is added NOT NULL with no DEFAULT, which fails on any table \
                 that already holds a row"
            ))
            .naming(format!("{table}.{column}"))),
        DdlStatement::SetNotNull { table, column } if !before.created_here.contains(table) => {
            Some(before
            .finding(Lint::NotNullWithoutDefault, statement, "SET NOT NULL", format!(
                    "{table}.{column} is made NOT NULL, which fails on any row that holds none"
                ))
            .naming(format!("{table}.{column}")))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::Lint;
    use crate::migration::lint::judge::test_support::{ORDERS, fired};

    #[test]
    fn a_column_added_or_made_not_null_with_nothing_to_fill_it_fires() {
        for sql in [
            "ALTER TABLE orders ADD COLUMN owner TEXT NOT NULL;",
            "ALTER TABLE orders ALTER COLUMN note SET NOT NULL;",
        ] {
            let found = fired(Lint::NotNullWithoutDefault, &[ORDERS, ("0002_owner", sql)]);
            assert_eq!(found.len(), 1, "{sql}");
            assert_eq!(found[0].table, "orders");
        }
    }

    #[test]
    fn a_default_or_a_table_made_in_the_same_file_does_not_fire() {
        for files in [
            vec![
                ORDERS,
                (
                    "0002_owner",
                    "ALTER TABLE orders ADD COLUMN owner TEXT NOT NULL DEFAULT '';",
                ),
            ],
            vec![(
                "0001_orders",
                "CREATE TABLE orders (id UUID NOT NULL, note TEXT, PRIMARY KEY (id HASH));\n\
                 ALTER TABLE orders ALTER COLUMN note SET NOT NULL;",
            )],
            vec![(
                "0001_orders",
                "CREATE TABLE orders (id UUID NOT NULL, PRIMARY KEY (id HASH));\n\
                 ALTER TABLE orders ADD COLUMN owner TEXT NOT NULL;",
            )],
        ] {
            assert!(fired(Lint::NotNullWithoutDefault, &files).is_empty());
        }
    }
}
