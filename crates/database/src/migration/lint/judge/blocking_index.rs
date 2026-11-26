//! `blocking_index`: an index built with writes held on a table that may hold rows.

use super::Before;
use crate::migration::ddl::DdlStatement;
use crate::migration::lint::{Lint, LintFinding};

/// Fires on `CREATE INDEX NONCONCURRENTLY` on a table its file did not create.
pub(crate) fn judge(before: &Before<'_>, statement: &DdlStatement) -> Option<LintFinding> {
    let DdlStatement::CreateIndex {
        name,
        table,
        online: false,
        ..
    } = statement
    else {
        return None;
    };
    (!before.created_here.contains(table)).then(|| {
        before
            .finding(
                Lint::BlockingIndex,
                statement,
                "CREATE INDEX",
                format!(
                    "{name} is built NONCONCURRENTLY on {table}, so its writes wait for the build"
                ),
            )
            .naming(name.clone())
    })
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::Lint;
    use crate::migration::lint::judge::test_support::{ORDERS, fired};

    #[test]
    fn an_index_built_with_writes_held_on_a_table_with_rows_fires() {
        let found = fired(
            Lint::BlockingIndex,
            &[
                ORDERS,
                (
                    "0002_by_note",
                    "CREATE INDEX NONCONCURRENTLY orders_by_note ON orders (note HASH);",
                ),
            ],
        );

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].table, "orders");
    }

    #[test]
    fn an_online_index_or_one_on_a_table_its_file_made_does_not_fire() {
        let online = [
            ORDERS,
            (
                "0002_by_note",
                "CREATE INDEX orders_by_note ON orders (note HASH);",
            ),
        ];
        let made_here = [(
            "0001_orders",
            "CREATE TABLE orders (id UUID NOT NULL, note TEXT, PRIMARY KEY (id HASH));\n\
             CREATE INDEX NONCONCURRENTLY orders_by_note ON orders (note HASH);",
        )];

        assert!(fired(Lint::BlockingIndex, &online).is_empty());
        assert!(fired(Lint::BlockingIndex, &made_here).is_empty());
    }
}
