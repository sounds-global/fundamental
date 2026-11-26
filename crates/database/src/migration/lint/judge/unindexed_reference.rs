//! `unindexed_reference`: a reference to another record that no key or index leads with.

use crate::migration::lint::facts::Facts;
use crate::migration::lint::{Lint, LintFinding};

/// Fires on a `UUID` column `<x>_id` leading no key or index, where `<x>s` or `<x>` names a
/// table of the set. Read from the name alone, so it only warns.
pub(crate) fn judge(facts: &Facts) -> Vec<LintFinding> {
    let mut found = Vec::new();
    for (name, table) in &facts.tables {
        let leading = table.leading_columns();
        for column in table.columns.values() {
            let Some(record) = column.name.strip_suffix("_id") else {
                continue;
            };
            if record.is_empty()
                || column.data_type != "UUID"
                || leading.contains(column.name.as_str())
            {
                continue;
            }
            let Some(referred) = [format!("{record}s"), record.to_owned()]
                .into_iter()
                .find(|other| other != name && facts.tables.contains_key(other))
            else {
                continue;
            };
            found.push(
                LintFinding::new(
                    Lint::UnindexedReference,
                    name.clone(),
                    format!("{name}.{}", column.name),
                    format!(
                        "{name}.{} refers to {referred} and leads no key and no index",
                        column.name
                    ),
                )
                .in_file(table.created_in.clone()),
            );
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::Lint;
    use crate::migration::lint::judge::test_support::{ORDERS, fired};

    const LINES: (&str, &str) = (
        "0002_order_lines",
        "CREATE TABLE order_lines (id UUID NOT NULL, order_id UUID NOT NULL, \
         PRIMARY KEY (id HASH));",
    );

    #[test]
    fn a_reference_to_a_table_of_the_set_that_leads_nothing_fires() {
        let found = fired(Lint::UnindexedReference, &[ORDERS, LINES]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].object, "order_lines.order_id");
        assert!(found[0].at.contains("refers to orders"));
        assert_eq!(found[0].migration.as_deref(), Some("0002_order_lines"));
    }

    #[test]
    fn an_indexed_reference_or_a_name_that_refers_to_no_table_does_not_fire() {
        let indexed = (
            "0003_by_order",
            "CREATE INDEX order_lines_by_order ON order_lines (order_id HASH);",
        );

        assert!(fired(Lint::UnindexedReference, &[ORDERS, LINES, indexed]).is_empty());
        assert!(fired(Lint::UnindexedReference, &[LINES]).is_empty());
    }
}
