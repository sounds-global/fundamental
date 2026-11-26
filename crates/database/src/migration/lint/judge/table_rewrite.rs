//! `table_rewrite`: a change the engine makes by writing every row of the table again.

use super::Before;
use crate::migration::ddl::DdlStatement;
use crate::migration::lint::{Lint, LintFinding};

/// Fires on `ALTER COLUMN … TYPE`, and `ADD COLUMN` with a volatile default, on a table its
/// file did not create.
pub(crate) fn judge(before: &Before<'_>, statement: &DdlStatement) -> Option<LintFinding> {
    let (table, column, verb, at) = match statement {
        DdlStatement::AlterColumnType { table, column, to } => (
            table,
            column,
            "ALTER COLUMN TYPE",
            format!("{table}.{column} is retyped to {to}, which writes every row again"),
        ),
        DdlStatement::AddColumn {
            table,
            column,
            default: Some(default),
            ..
        } if volatile(default) => (
            table,
            column,
            "ADD COLUMN",
            format!(
                "{table}.{column} is added with the volatile default {default}, which writes \
                 every row again"
            ),
        ),
        _ => return None,
    };
    (!before.created_here.contains(table)).then(|| {
        before
            .finding(Lint::TableRewrite, statement, verb, at)
            .naming(format!("{table}.{column}"))
    })
}

fn volatile(default: &str) -> bool {
    let default = default.to_ascii_lowercase();
    [
        "random()",
        "gen_random_uuid()",
        "uuid_generate_v4()",
        "clock_timestamp()",
        "nextval(",
    ]
    .iter()
    .any(|call| default.contains(call))
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::Lint;
    use crate::migration::lint::judge::test_support::{ORDERS, fired};

    #[test]
    fn a_column_retyped_on_a_table_with_rows_fires() {
        let found = fired(
            Lint::TableRewrite,
            &[
                ORDERS,
                (
                    "0002_total",
                    "ALTER TABLE orders ALTER COLUMN total TYPE NUMERIC;",
                ),
            ],
        );

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].object, "orders.total");
    }

    #[test]
    fn a_column_added_with_a_value_a_row_on_a_table_with_rows_fires_and_a_constant_does_not() {
        let volatile = fired(
            Lint::TableRewrite,
            &[
                ORDERS,
                (
                    "0002_token",
                    "ALTER TABLE orders ADD COLUMN token UUID NOT NULL DEFAULT gen_random_uuid();",
                ),
            ],
        );
        let constant = fired(
            Lint::TableRewrite,
            &[
                ORDERS,
                (
                    "0002_kind",
                    "ALTER TABLE orders ADD COLUMN kind TEXT NOT NULL DEFAULT 'x';",
                ),
            ],
        );

        assert_eq!(volatile.len(), 1);
        assert!(
            volatile[0]
                .at
                .contains("volatile default gen_random_uuid()")
        );
        assert!(constant.is_empty());
    }

    #[test]
    fn a_column_retyped_beside_the_table_it_is_on_does_not_fire() {
        let created = "CREATE TABLE orders (id UUID NOT NULL, total INT8, PRIMARY KEY (id HASH));\n\
                       ALTER TABLE orders ALTER COLUMN total TYPE NUMERIC;";

        assert!(fired(Lint::TableRewrite, &[("0001_orders", created)]).is_empty());
    }
}
