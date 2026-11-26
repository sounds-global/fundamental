//! `ddl_beside_data`: one file that changes the schema and moves rows.

use crate::migration::ddl::DdlStatement;
use crate::migration::lint::{Lint, LintFinding};

/// Fires on each row-moving statement in a file that also changes the schema: each runs as
/// its own step, so a failure between them leaves the two out of step.
pub(crate) fn judge(migration: &str, statements: &[DdlStatement]) -> Vec<LintFinding> {
    let changes_schema = statements.iter().any(|statement| match statement {
        DdlStatement::Data { .. } | DdlStatement::CreateRole { .. } => false,
        DdlStatement::Other { lead } => ["ALTER", "CREATE", "DROP", "COMMENT"]
            .iter()
            .any(|verb| lead.starts_with(verb)),
        _ => true,
    });
    if !changes_schema {
        return Vec::new();
    }
    statements
        .iter()
        .filter_map(|statement| match statement {
            DdlStatement::Data { verb, table, .. } => Some(
                LintFinding::new(
                    Lint::DdlBesideData,
                    table.clone(),
                    table.clone(),
                    format!("{verb} on {table} is in a file that also changes the schema"),
                )
                .in_file(migration)
                .by(verb),
            ),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::Lint;
    use crate::migration::lint::judge::test_support::{ORDERS, fired};

    #[test]
    fn a_file_that_changes_the_schema_and_moves_rows_fires() {
        let found = fired(
            Lint::DdlBesideData,
            &[
                ORDERS,
                (
                    "0002_owner",
                    "ALTER TABLE orders ADD COLUMN owner TEXT;\n\
                     UPDATE orders SET owner = 'nobody';",
                ),
            ],
        );

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].migration.as_deref(), Some("0002_owner"));
    }

    #[test]
    fn a_file_of_data_alone_or_of_schema_alone_does_not_fire() {
        let data = [ORDERS, ("0002_fill", "UPDATE orders SET note = '';")];

        assert!(fired(Lint::DdlBesideData, &data).is_empty());
        assert!(fired(Lint::DdlBesideData, &[ORDERS]).is_empty());
    }
}
