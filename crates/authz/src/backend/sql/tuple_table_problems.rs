//! [`tuple_table_problems`]: every way a live tuple table differs from the shipped set.

use fundamental_core::model::SqlDb;
use fundamental_db::migration::ddl::{DdlStatement, KeyDeclaration, recognise};
use fundamental_db::migration::{ModelSchema, split_statements};
use fundamental_sql::migration::TableCatalog;

use super::relation_tuple_row::RelationTupleRow;
use super::sql_tuple_migrations::SQL_TUPLE_MIGRATIONS;
use crate::error::AuthzError;

/// The collation every text column of the set compares with.
const DEFAULT_COLLATION: &str = "default";

/// Compares `catalog` with the table [`SQL_TUPLE_MIGRATIONS`] creates, and names every
/// difference.
///
/// The expected shape comes from [`RelationTupleRow`]'s model and the shipped migration text.
/// Anything that changes what a statement does counts, including any undeclared index,
/// constraint, trigger, rule, policy or partitioning.
///
/// # Errors
///
/// [`AuthzError::SchemaSetup`] if the shipped migration text does not read.
pub(crate) fn tuple_table_problems(catalog: &TableCatalog) -> Result<Vec<String>, AuthzError> {
    let shipped = shipped_keys()?;
    let mut problems = Vec::new();
    columns(catalog, &mut problems);
    keys(catalog, &shipped, &mut problems);
    attachments(catalog, &mut problems);
    Ok(problems)
}

/// The keys the shipped set declares: the primary key, then each index by name.
struct ShippedKeys {
    primary_key: KeyDeclaration,
    indexes: Vec<(String, KeyDeclaration)>,
}

fn shipped_keys() -> Result<ShippedKeys, AuthzError> {
    let mut primary_key = None;
    let mut indexes = Vec::new();
    for migration in SQL_TUPLE_MIGRATIONS.migrations() {
        for statement in split_statements(migration.up()) {
            match recognise(&statement).map_err(|e| AuthzError::SchemaSetup(e.to_string()))? {
                DdlStatement::CreateTable {
                    primary_key: key, ..
                } => primary_key = key,
                DdlStatement::CreateIndex { name, key, .. } => indexes.push((name, key)),
                _ => {}
            }
        }
    }
    let primary_key = primary_key.ok_or_else(|| {
        AuthzError::SchemaSetup("the shipped tuple table declares no primary key".to_owned())
    })?;
    Ok(ShippedKeys {
        primary_key,
        indexes,
    })
}

fn columns(catalog: &TableCatalog, problems: &mut Vec<String>) {
    let model = <RelationTupleRow as ModelSchema<SqlDb>>::table_schema();
    for name in &model.column_order {
        let Some(expected) = model.columns.get(name) else {
            continue;
        };
        let Some(live) = catalog.column(name) else {
            problems.push(format!("column {name} is missing"));
            continue;
        };
        if !live.data_type.eq_ignore_ascii_case(&expected.data_type) {
            problems.push(format!(
                "column {name} is {}, the set declares {}",
                live.data_type, expected.data_type
            ));
        }
        if live.nullable != expected.nullable {
            problems.push(format!(
                "column {name} is {}, the set declares it {}",
                null_word(live.nullable),
                null_word(expected.nullable)
            ));
        }
        if live.default != expected.default {
            problems.push(format!(
                "column {name} defaults to {}, the set declares {}",
                live.default.as_deref().unwrap_or("nothing"),
                expected.default.as_deref().unwrap_or("no default")
            ));
        }
        if let Some(collation) = live
            .collation
            .as_deref()
            .filter(|c| *c != DEFAULT_COLLATION)
        {
            problems.push(format!(
                "column {name} collates as {collation}, the set uses the default"
            ));
        }
    }
    for live in &catalog.columns {
        if !model.columns.contains_key(&live.name) {
            problems.push(format!("column {} is not in the set", live.name));
        }
    }
}

fn keys(catalog: &TableCatalog, shipped: &ShippedKeys, problems: &mut Vec<String>) {
    match catalog.primary_key() {
        None => problems.push("the table has no primary key".to_owned()),
        Some(live) if live.key.as_ref() != Some(&shipped.primary_key) => problems.push(format!(
            "the primary key is {}, the set declares ({})",
            live.definition, shipped.primary_key
        )),
        Some(live) if !live.usable => {
            problems.push("the primary key is not usable: its build did not finish".to_owned());
        }
        Some(_) => {}
    }
    for (name, key) in &shipped.indexes {
        let Some(live) = catalog.index(name) else {
            problems.push(format!("index {name} is missing"));
            continue;
        };
        if live.key.as_ref() != Some(key) {
            problems.push(format!(
                "index {name} is {}, the set declares ({key})",
                live.definition
            ));
        }
        for (wrong, what) in [
            (live.unique, "unique"),
            (live.partial, "partial"),
            (live.expressions, "over expressions"),
            (live.includes, "covering (INCLUDE)"),
        ] {
            if wrong {
                problems.push(format!("index {name} is {what}; the set's is not"));
            }
        }
        if !live.usable {
            problems.push(format!(
                "index {name} is not usable: its build did not finish"
            ));
        }
        if live.method != "lsm" {
            problems.push(format!(
                "index {name} uses {}, the set's uses lsm",
                live.method
            ));
        }
    }
    for live in &catalog.indexes {
        let shipped_index = shipped.indexes.iter().any(|(name, _)| *name == live.name);
        if !live.primary && !shipped_index {
            problems.push(format!(
                "index {} is not in the set: {}",
                live.name, live.definition
            ));
        }
    }
}

fn attachments(catalog: &TableCatalog, problems: &mut Vec<String>) {
    for constraint in catalog
        .constraints
        .iter()
        .filter(|c| c.kind != 'p' || c.table != catalog.table)
    {
        if constraint.table == catalog.table {
            problems.push(format!(
                "constraint {} is not in the set: {}",
                constraint.name, constraint.definition
            ));
        } else {
            problems.push(format!(
                "constraint {} on {} references the table: {}",
                constraint.name, constraint.table, constraint.definition
            ));
        }
    }
    for trigger in &catalog.triggers {
        problems.push(format!("trigger {trigger} is not in the set"));
    }
    for rule in &catalog.rules {
        problems.push(format!("rule {rule} is not in the set"));
    }
    if catalog.row_security || catalog.row_security_forced {
        problems.push("row-level security is enabled; the set's table has none".to_owned());
    }
    for policy in &catalog.policies {
        problems.push(format!("policy {policy} is not in the set"));
    }
    if catalog.partitioned {
        problems.push("the table is partitioned; the set's is not".to_owned());
    }
    for related in &catalog.inheritance {
        problems.push(format!(
            "the table inherits from, or is inherited by, {related}"
        ));
    }
    if catalog.colocated {
        problems.push(
            "the table shares its tablets with other tables; the set's is split across its own"
                .to_owned(),
        );
    }
}

fn null_word(nullable: bool) -> &'static str {
    if nullable { "nullable" } else { "NOT NULL" }
}

#[cfg(test)]
mod tests {
    use fundamental_db::migration::ddl::{DdlStatement, recognise};
    use fundamental_sql::migration::{
        CatalogColumn, CatalogConstraint, CatalogIndex, RoleAccess, TableCatalog,
    };

    use super::tuple_table_problems;

    fn index(name: &str, definition: &str, primary: bool) -> CatalogIndex {
        let DdlStatement::CreateIndex { key, .. } = recognise(definition).expect("reads") else {
            panic!("an index");
        };
        CatalogIndex {
            name: name.to_owned(),
            key: Some(key),
            definition: definition.to_owned(),
            primary,
            unique: primary,
            partial: false,
            condition: None,
            expressions: false,
            includes: false,
            include: Vec::new(),
            method: "lsm".to_owned(),
            usable: true,
        }
    }

    fn text(name: &str, nullable: bool) -> CatalogColumn {
        CatalogColumn {
            name: name.to_owned(),
            data_type: "TEXT".to_owned(),
            nullable,
            default: None,
            collation: Some("default".to_owned()),
        }
    }

    fn migrated() -> TableCatalog {
        TableCatalog {
            table: "authz_relation_tuple".to_owned(),
            columns: vec![
                text("tuple_key", false),
                text("object_ns", false),
                text("object_id", false),
                text("relation", false),
                text("subject_ns", false),
                text("subject_id", false),
                text("subject_relation", false),
                text("caveat_name", true),
                text("caveat_context", true),
            ],
            indexes: vec![
                index(
                    "authz_relation_tuple_forward_idx",
                    "CREATE INDEX authz_relation_tuple_forward_idx ON public.authz_relation_tuple \
                     USING lsm ((object_ns, object_id, relation) HASH)",
                    false,
                ),
                index(
                    "authz_relation_tuple_pkey",
                    "CREATE UNIQUE INDEX authz_relation_tuple_pkey ON public.authz_relation_tuple \
                     USING lsm (tuple_key HASH)",
                    true,
                ),
                index(
                    "authz_relation_tuple_reverse_idx",
                    "CREATE INDEX authz_relation_tuple_reverse_idx ON public.authz_relation_tuple \
                     USING lsm ((subject_ns, subject_id, subject_relation) HASH, object_ns ASC, \
                     relation ASC)",
                    false,
                ),
            ],
            constraints: vec![CatalogConstraint {
                name: "authz_relation_tuple_pkey".to_owned(),
                table: "authz_relation_tuple".to_owned(),
                kind: 'p',
                definition: "PRIMARY KEY ((tuple_key) HASH)".to_owned(),
            }],
            triggers: Vec::new(),
            rules: Vec::new(),
            row_security: false,
            row_security_forced: false,
            policies: Vec::new(),
            readers: std::collections::BTreeMap::new(),
            partitioned: false,
            inheritance: Vec::new(),
            colocated: false,
            tablets: None,
            writers: std::collections::BTreeSet::new(),
            access: RoleAccess::default(),
        }
    }

    fn problems(catalog: &TableCatalog) -> Vec<String> {
        tuple_table_problems(catalog).expect("the shipped set reads")
    }

    #[test]
    fn the_migrated_table_has_no_problem() {
        assert_eq!(problems(&migrated()), Vec::<String>::new());
    }

    #[test]
    fn an_undeclared_forward_key_is_named() {
        let mut catalog = migrated();
        catalog.indexes[0] = index(
            "authz_relation_tuple_forward_idx",
            "CREATE INDEX authz_relation_tuple_forward_idx ON public.authz_relation_tuple \
             USING lsm (object_ns HASH, object_id ASC, relation ASC)",
            false,
        );
        let found = problems(&catalog);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("index authz_relation_tuple_forward_idx is CREATE INDEX"),
            "{found:?}"
        );
    }

    #[test]
    fn a_unique_or_partial_index_with_the_right_key_is_named() {
        let mut catalog = migrated();
        catalog.indexes[0].unique = true;
        catalog.indexes[2].partial = true;
        let found = problems(&catalog);
        assert_eq!(
            found,
            [
                "index authz_relation_tuple_forward_idx is unique; the set's is not",
                "index authz_relation_tuple_reverse_idx is partial; the set's is not",
            ]
        );
    }

    #[test]
    fn a_missing_index_and_column_are_both_named() {
        let mut catalog = migrated();
        catalog.indexes.remove(2);
        catalog
            .columns
            .retain(|column| column.name != "caveat_context");
        assert_eq!(
            problems(&catalog),
            [
                "column caveat_context is missing",
                "index authz_relation_tuple_reverse_idx is missing",
            ]
        );
    }

    #[test]
    fn a_column_retyped_defaulted_collated_or_extra_is_named() {
        let mut catalog = migrated();
        catalog.columns[1].data_type = "CHARACTER VARYING(64)".to_owned();
        catalog.columns[2].nullable = true;
        catalog.columns[3].default = Some("'viewer'::text".to_owned());
        catalog.columns[4].collation = Some("C".to_owned());
        catalog.columns.push(text("tenant", true));
        assert_eq!(
            problems(&catalog),
            [
                "column object_ns is CHARACTER VARYING(64), the set declares TEXT",
                "column object_id is nullable, the set declares it NOT NULL",
                "column relation defaults to 'viewer'::text, the set declares no default",
                "column subject_ns collates as C, the set uses the default",
                "column tenant is not in the set",
            ]
        );
    }

    #[test]
    fn what_rewrites_a_statement_is_named() {
        let mut catalog = migrated();
        catalog.triggers.push("rewrite_grants".to_owned());
        catalog.rules.push("swallow_deletes".to_owned());
        catalog.row_security = true;
        catalog.policies.push("hide_from_delete".to_owned());
        catalog.constraints.push(CatalogConstraint {
            name: "one_owner".to_owned(),
            table: "authz_relation_tuple".to_owned(),
            kind: 'c',
            definition: "CHECK (relation <> 'owner'::text)".to_owned(),
        });
        catalog.constraints.push(CatalogConstraint {
            name: "pinned_grant".to_owned(),
            table: "pins".to_owned(),
            kind: 'f',
            definition: "FOREIGN KEY (k) REFERENCES authz_relation_tuple(tuple_key)".to_owned(),
        });
        catalog.partitioned = true;
        catalog
            .inheritance
            .push("authz_relation_tuple_p0".to_owned());
        catalog.colocated = true;
        let mut extra = index(
            "one_edge_per_relation",
            "CREATE UNIQUE INDEX one_edge_per_relation ON public.authz_relation_tuple \
             USING lsm ((object_ns, object_id, relation) HASH)",
            false,
        );
        extra.unique = true;
        catalog.indexes.push(extra);
        assert_eq!(
            problems(&catalog),
            [
                "index one_edge_per_relation is not in the set: CREATE UNIQUE INDEX \
                 one_edge_per_relation ON public.authz_relation_tuple USING lsm ((object_ns, \
                 object_id, relation) HASH)",
                "constraint one_owner is not in the set: CHECK (relation <> 'owner'::text)",
                "constraint pinned_grant on pins references the table: FOREIGN KEY (k) \
                 REFERENCES authz_relation_tuple(tuple_key)",
                "trigger rewrite_grants is not in the set",
                "rule swallow_deletes is not in the set",
                "row-level security is enabled; the set's table has none",
                "policy hide_from_delete is not in the set",
                "the table is partitioned; the set's is not",
                "the table inherits from, or is inherited by, authz_relation_tuple_p0",
                "the table shares its tablets with other tables; the set's is split across its own",
            ]
        );
    }

    #[test]
    fn any_index_the_set_does_not_declare_is_named() {
        let mut catalog = migrated();
        let mut over_a_function = index(
            "by_caveat",
            "CREATE INDEX by_caveat ON public.authz_relation_tuple USING lsm (caveat_name HASH)",
            false,
        );
        over_a_function.definition =
            "CREATE INDEX by_caveat ON public.authz_relation_tuple USING lsm (f(caveat_name) HASH)"
                .to_owned();
        over_a_function.key = None;
        over_a_function.expressions = true;
        catalog.indexes.push(over_a_function);
        assert_eq!(
            problems(&catalog),
            [
                "index by_caveat is not in the set: CREATE INDEX by_caveat ON \
              public.authz_relation_tuple USING lsm (f(caveat_name) HASH)"
            ]
        );
    }

    #[test]
    fn an_index_whose_build_did_not_finish_is_named() {
        let mut catalog = migrated();
        catalog.indexes[0].usable = false;
        catalog.indexes[1].usable = false;
        assert_eq!(
            problems(&catalog),
            [
                "the primary key is not usable: its build did not finish",
                "index authz_relation_tuple_forward_idx is not usable: its build did not finish",
            ]
        );
    }

    #[test]
    fn a_primary_key_sharded_otherwise_is_named() {
        let mut catalog = migrated();
        catalog.indexes[1] = index(
            "authz_relation_tuple_pkey",
            "CREATE UNIQUE INDEX authz_relation_tuple_pkey ON public.authz_relation_tuple \
             USING lsm (tuple_key ASC)",
            true,
        );
        let found = problems(&catalog);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("the primary key is"), "{found:?}");
    }
}
