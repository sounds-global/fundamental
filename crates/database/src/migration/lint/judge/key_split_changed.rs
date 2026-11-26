//! `key_split_changed`: a primary key put on a table that already holds rows.

use super::Before;
use crate::migration::ddl::DdlStatement;
use crate::migration::lint::{Lint, LintFinding};

/// Fires on `ADD PRIMARY KEY` on a table its file did not create, which rewrites every row.
pub(crate) fn judge(before: &Before<'_>, statement: &DdlStatement) -> Option<LintFinding> {
    let DdlStatement::AddPrimaryKey { table, .. } = statement else {
        return None;
    };
    (!before.created_here.contains(table)).then(|| {
        before.finding(
            Lint::KeySplitChanged,
            statement,
            "ADD PRIMARY KEY",
            format!("{table} is given a new primary key, which writes every row again"),
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::Lint;
    use crate::migration::lint::judge::test_support::{ORDERS, fired};

    #[test]
    fn a_key_added_to_a_table_with_rows_fires() {
        let found = fired(
            Lint::KeySplitChanged,
            &[
                ORDERS,
                (
                    "0002_orders_key",
                    "ALTER TABLE orders DROP CONSTRAINT orders_pkey;\n\
                     ALTER TABLE orders ADD PRIMARY KEY (at ASC, id ASC);",
                ),
            ],
        );

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].table, "orders");
        assert!(found[0].at.contains("new primary key"), "{}", found[0].at);
    }

    #[test]
    fn a_key_added_to_a_table_its_file_made_does_not_fire() {
        let made_here = [(
            "0001_orders",
            "CREATE TABLE orders (id UUID NOT NULL, note TEXT);\n\
             ALTER TABLE orders ADD PRIMARY KEY (id HASH);",
        )];

        assert!(fired(Lint::KeySplitChanged, &made_here).is_empty());
        assert!(fired(Lint::KeySplitChanged, &[ORDERS]).is_empty());
    }
}
