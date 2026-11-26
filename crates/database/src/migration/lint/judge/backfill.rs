//! `backfill`: a migration that writes the rows of a table it did not create.

use super::Before;
use crate::migration::ddl::DdlStatement;
use crate::migration::lint::{Lint, LintFinding};

/// Fires on a row-moving statement into a table its file did not create.
pub(crate) fn judge(before: &Before<'_>, statement: &DdlStatement) -> Option<LintFinding> {
    let DdlStatement::Data { verb, table, .. } = statement else {
        return None;
    };
    (!before.created_here.contains(table)).then(|| {
        before
            .finding(
                Lint::Backfill,
                statement,
                verb,
                format!("{verb} writes the rows of {table} inside the migration"),
            )
            .naming(table.clone())
    })
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::Lint;
    use crate::migration::lint::judge::test_support::{ORDERS, fired};

    #[test]
    fn rows_written_in_a_table_that_holds_them_already_fire() {
        for sql in [
            "UPDATE orders SET note = '' WHERE note IS NULL;",
            "DELETE FROM orders WHERE total = 0;",
            "INSERT INTO orders (id, total) SELECT id, 0 FROM orders;",
        ] {
            let found = fired(Lint::Backfill, &[ORDERS, ("0002_fill", sql)]);
            assert_eq!(found.len(), 1, "{sql}");
            assert_eq!(found[0].table, "orders");
        }
    }

    #[test]
    fn a_seed_or_rows_of_a_table_made_in_the_same_file_do_not_fire() {
        let seed = [
            ORDERS,
            (
                "0002_seed",
                "INSERT INTO orders (id, total) VALUES ('a', 1);",
            ),
        ];
        let made_here = [(
            "0001_kinds",
            "CREATE TABLE kinds (name TEXT NOT NULL, PRIMARY KEY (name HASH));\n\
             INSERT INTO kinds (name) SELECT 'news';",
        )];

        assert!(fired(Lint::Backfill, &seed).is_empty());
        assert!(fired(Lint::Backfill, &made_here).is_empty());
    }
}
