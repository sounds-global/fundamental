//! `destructive`: a statement that removes data, a schema object or a name running code reads.

use super::Before;
use crate::migration::ddl::DdlStatement;
use crate::migration::lint::{Lint, LintFinding};

/// Fires on a `DROP`, a `TRUNCATE`, and an `ALTER TABLE` that drops a column or constraint or
/// renames a column; its declarations are its acceptance.
pub(crate) fn judge(before: &Before<'_>, statement: &DdlStatement) -> Option<LintFinding> {
    let DdlStatement::Destructive { verb, object } = statement else {
        return None;
    };
    Some(
        before
            .finding(
                Lint::Destructive,
                statement,
                verb,
                format!("{verb} {object} removes data or a name running code reads"),
            )
            .naming(object.clone()),
    )
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::Lint;
    use crate::migration::lint::judge::test_support::{ORDERS, fired};

    #[test]
    fn a_drop_fires_naming_its_verb_object_and_table() {
        let found = fired(
            Lint::Destructive,
            &[
                ORDERS,
                ("0002_note", "ALTER TABLE orders DROP COLUMN note;"),
            ],
        );

        assert_eq!(found.len(), 1);
        assert_eq!(
            (
                found[0].table.as_str(),
                found[0].verb.as_str(),
                found[0].object.as_str()
            ),
            ("orders", "DROP COLUMN", "orders.note")
        );
        assert_eq!(found[0].migration.as_deref(), Some("0002_note"));
    }

    #[test]
    fn adding_and_altering_without_removing_does_not_fire() {
        assert!(
            fired(
                Lint::Destructive,
                &[
                    ORDERS,
                    ("0002_more", "ALTER TABLE orders ADD COLUMN more TEXT;")
                ]
            )
            .is_empty()
        );
    }
}
