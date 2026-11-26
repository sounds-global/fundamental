//! `tenant_not_leading`: a tenant's table whose key does not lead with the tenant.

use crate::migration::lint::facts::Facts;
use crate::migration::lint::{Lint, LintFinding, Lints};

/// Fires on a table holding the project's tenant column whose key does not lead with it.
pub(crate) fn judge(facts: &Facts, lints: &Lints) -> Vec<LintFinding> {
    let Some(tenant) = lints.tenant_column() else {
        return Vec::new();
    };
    facts
        .tables
        .iter()
        .filter(|(_, table)| table.column(tenant).is_some())
        .filter(|(_, table)| {
            table
                .key
                .first()
                .and_then(|(columns, _)| columns.first())
                .is_none_or(|lead| lead != tenant)
        })
        .map(|(name, table)| {
            LintFinding::new(
                Lint::TenantNotLeading,
                name.clone(),
                name.clone(),
                format!("{name} holds {tenant} and its key does not lead with it"),
            )
            .in_file(table.created_in.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::migration::lint::judge::test_support::fired_under;
    use crate::migration::lint::{Lint, Lints};

    fn tenant() -> Lints {
        Lints::default().tenant("organisation_id")
    }

    #[test]
    fn a_tenants_table_led_by_another_column_fires() {
        let files = [(
            "0001_projects",
            "CREATE TABLE projects (id UUID NOT NULL, organisation_id UUID NOT NULL, \
             PRIMARY KEY (id HASH, organisation_id ASC));",
        )];

        assert_eq!(
            fired_under(&tenant(), Lint::TenantNotLeading, &files).len(),
            1
        );
    }

    #[test]
    fn a_table_led_by_its_tenant_or_without_one_or_a_project_with_no_tenant_does_not_fire() {
        let led = [(
            "0001_projects",
            "CREATE TABLE projects (organisation_id UUID NOT NULL, id UUID NOT NULL, \
             PRIMARY KEY (organisation_id HASH, id ASC));",
        )];
        let unled = [(
            "0001_projects",
            "CREATE TABLE projects (id UUID NOT NULL, organisation_id UUID NOT NULL, \
             PRIMARY KEY (id HASH));",
        )];
        let none = [(
            "0001_kinds",
            "CREATE TABLE kinds (name TEXT NOT NULL, PRIMARY KEY (name HASH));",
        )];

        assert!(fired_under(&tenant(), Lint::TenantNotLeading, &led).is_empty());
        assert!(fired_under(&tenant(), Lint::TenantNotLeading, &none).is_empty());
        assert!(fired_under(&Lints::default(), Lint::TenantNotLeading, &unled).is_empty());
    }
}
