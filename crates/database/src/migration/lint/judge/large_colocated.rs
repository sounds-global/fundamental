//! `large_colocated`: a colocated table that is expected to grow large.

use crate::migration::lint::facts::Facts;
use crate::migration::lint::{Lint, LintFinding, Lints};

const OUTBOX_COLUMN: &str = "outbox_seal";

/// Fires, in a colocated database, on a colocated table that grows with time or with tenants.
pub(crate) fn judge(facts: &Facts, lints: &Lints) -> Vec<LintFinding> {
    if !lints.is_colocated() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (name, table) in &facts.tables {
        if table.colocated == Some(false) {
            continue;
        }
        let key_columns = || table.key.iter().flat_map(|(columns, _)| columns);
        let why = if table.column(OUTBOX_COLUMN).is_some() {
            Some("is an outbox".to_owned())
        } else if let Some(column) =
            key_columns().find(|column| table.column(column).is_some_and(|c| c.only_increases()))
        {
            Some(format!("is keyed on {column}, a value that only increases"))
        } else {
            lints
                .tenant_column()
                .filter(|tenant| key_columns().next().is_some_and(|lead| lead == tenant))
                .map(|tenant| format!("is keyed by its tenant, {tenant}"))
        };
        if let Some(why) = why {
            found.push(
                LintFinding::new(
                    Lint::LargeColocated,
                    name.clone(),
                    name.clone(),
                    format!("{name} {why}, and shares the colocated tablet"),
                )
                .in_file(table.created_in.clone()),
            );
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::judge::test_support::fired_under;
    use crate::migration::lint::{Lint, Lints};

    const EVENTS: (&str, &str) = (
        "0001_events",
        "CREATE TABLE events (id UUID NOT NULL, at TIMESTAMPTZ NOT NULL, \
         PRIMARY KEY (id HASH, at ASC));",
    );

    #[test]
    fn a_colocated_table_keyed_on_time_an_outbox_or_a_tenants_fires() {
        let colocated = Lints::default().colocated(true).tenant("organisation_id");
        let outbox = (
            "0002_relay",
            "CREATE TABLE relay (id UUID NOT NULL, outbox_seal BYTEA, PRIMARY KEY (id HASH));",
        );
        let tenants = (
            "0003_projects",
            "CREATE TABLE projects (organisation_id UUID NOT NULL, PRIMARY KEY (organisation_id HASH));",
        );

        let found = fired_under(&colocated, Lint::LargeColocated, &[EVENTS, outbox, tenants]);

        assert_eq!(found.len(), 3, "{found:?}");
    }

    #[test]
    fn a_table_out_of_colocation_or_a_database_that_is_not_colocated_does_not_fire() {
        let apart = (
            "0001_events",
            "CREATE TABLE events (id UUID NOT NULL, at TIMESTAMPTZ NOT NULL, \
             PRIMARY KEY (id HASH, at ASC)) WITH (COLOCATION = false);",
        );
        let colocated = Lints::default().colocated(true);

        assert!(fired_under(&colocated, Lint::LargeColocated, &[apart]).is_empty());
        assert!(fired_under(&Lints::default(), Lint::LargeColocated, &[EVENTS]).is_empty());
    }
}
