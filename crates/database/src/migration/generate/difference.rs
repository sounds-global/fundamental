//! [`SchemaDifference`]: one exact way the schema found differs from the schema expected.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::{Snapshot, SnapshotColumn, SnapshotIndex, SnapshotTable};
use crate::migration::SchemaDiffer;
use crate::migration::render::key;
use crate::migration::table::condition_words;

/// The part of a table a [`SchemaDifference`] is in.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SchemaItem {
    /// The table itself: one side holds it and the other does not.
    Table,
    /// The column of this name.
    Column(String),
    /// The table's key, with how each part is split.
    Key,
    /// The index of this name.
    Index(String),
    /// The check of this name.
    Check(String),
    /// What the role of this name may read.
    Reader(String),
    /// Whether the table shares the colocated tablet.
    Colocation,
    /// The tablets the table is split into.
    Tablets,
    /// Whether the role of this name may write the table.
    Writer(String),
}

impl fmt::Display for SchemaItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table => f.write_str("table"),
            Self::Column(name) => write!(f, "column {name}"),
            Self::Key => f.write_str("key"),
            Self::Index(name) => write!(f, "index {name}"),
            Self::Check(name) => write!(f, "check {name}"),
            Self::Reader(role) => write!(f, "reader {role}"),
            Self::Colocation => f.write_str("colocation"),
            Self::Tablets => f.write_str("tablets"),
            Self::Writer(role) => write!(f, "writer {role}"),
        }
    }
}

/// One way the schema found differs from the schema expected: the table, the item in it,
/// and each side's value as SQL writes it, or `None` where that side lacks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDifference {
    /// The table it is in.
    pub table: String,
    /// What in the table differs.
    pub item: SchemaItem,
    /// The value expected, or `None` when the expected schema lacks it.
    pub expected: Option<String>,
    /// The value found, or `None` when the schema found lacks it.
    pub found: Option<String>,
}

impl fmt::Display for SchemaDifference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}  {}  expected {}  found {}",
            self.table,
            self.item,
            self.expected.as_deref().unwrap_or("none"),
            self.found.as_deref().unwrap_or("none"),
        )
    }
}

impl Snapshot {
    /// Every way `found` differs from this snapshot, table by table in name order; empty when the
    /// two agree.
    ///
    /// Values are compared as the engine means them, so a schema read back from a database equals
    /// the snapshot its files were generated with. Column order is not compared.
    #[must_use]
    pub fn differences(&self, found: &Self) -> Vec<SchemaDifference> {
        let names: BTreeSet<&String> = self.tables.keys().chain(found.tables.keys()).collect();
        let mut differences = Vec::new();
        for name in names {
            match (self.tables.get(name), found.tables.get(name)) {
                (Some(expected), Some(found)) => {
                    table_differences(name, expected, found, &mut differences);
                }
                (expected, found) => differences.push(SchemaDifference {
                    table: name.clone(),
                    item: SchemaItem::Table,
                    expected: expected.map(|_| "present".to_owned()),
                    found: found.map(|_| "present".to_owned()),
                }),
            }
        }
        differences
    }
}

fn table_differences(
    name: &str,
    expected: &SnapshotTable,
    found: &SnapshotTable,
    out: &mut Vec<SchemaDifference>,
) {
    let mut push = |item: SchemaItem, expected: Option<String>, found: Option<String>| {
        out.push(SchemaDifference {
            table: name.to_owned(),
            item,
            expected,
            found,
        });
    };

    let mut columns: Vec<&str> = expected.columns.iter().map(|c| c.name.as_str()).collect();
    columns.extend(
        found
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .filter(|column| expected.column(column).is_none()),
    );
    for column in columns {
        let (left, right) = (expected.column(column), found.column(column));
        if !same(left, right, same_column) {
            push(
                SchemaItem::Column(column.to_owned()),
                left.map(column_sql),
                right.map(column_sql),
            );
        }
    }

    if expected.key != found.key {
        let written = |parts: &[_]| (!parts.is_empty()).then(|| format!("({})", key(parts)));
        push(SchemaItem::Key, written(&expected.key), written(&found.key));
    }

    for index in union(expected.indexes.keys(), found.indexes.keys()) {
        let (left, right) = (expected.indexes.get(index), found.indexes.get(index));
        let alike =
            |a: &SnapshotIndex, b: &SnapshotIndex| a.schema(index).same_as(&b.schema(index));
        if !same(left, right, alike) {
            push(
                SchemaItem::Index(index.clone()),
                left.map(index_sql),
                right.map(index_sql),
            );
        }
    }

    expressions(
        &expected.checks,
        &found.checks,
        SchemaItem::Check,
        &mut push,
    );
    expressions(
        &expected.readers,
        &found.readers,
        SchemaItem::Reader,
        &mut push,
    );
    placement(expected, found, &mut push);
}

/// Placement and writers, compared only where the expected side says them; an extra writer
/// on the found side is no difference.
fn placement(
    expected: &SnapshotTable,
    found: &SnapshotTable,
    push: &mut impl FnMut(SchemaItem, Option<String>, Option<String>),
) {
    let said = |colocated: Option<bool>| {
        colocated.map(|c| if c { "colocated" } else { "apart" }.to_owned())
    };
    if expected.colocated.is_some() && expected.colocated != found.colocated {
        push(
            SchemaItem::Colocation,
            said(expected.colocated),
            said(found.colocated),
        );
    }
    if expected.tablets.is_some() && expected.tablets != found.tablets {
        let count = |tablets: Option<u32>| tablets.map(|n| n.to_string());
        push(
            SchemaItem::Tablets,
            count(expected.tablets),
            count(found.tablets),
        );
    }
    for role in expected.writers.difference(&found.writers) {
        push(
            SchemaItem::Writer(role.clone()),
            Some("SELECT, INSERT, UPDATE, DELETE".to_owned()),
            None,
        );
    }
}

/// Each name either side holds whose expression the other lacks or means otherwise.
fn expressions(
    expected: &BTreeMap<String, String>,
    found: &BTreeMap<String, String>,
    item: fn(String) -> SchemaItem,
    push: &mut impl FnMut(SchemaItem, Option<String>, Option<String>),
) {
    for name in union(expected.keys(), found.keys()) {
        let (left, right) = (expected.get(name), found.get(name));
        if !same(left, right, |a, b| {
            expression_words(a) == expression_words(b)
        }) {
            push(item(name.clone()), left.cloned(), right.cloned());
        }
    }
}

fn same<T>(left: Option<&T>, right: Option<&T>, alike: impl Fn(&T, &T) -> bool) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => alike(left, right),
        _ => false,
    }
}

fn union<'a>(
    left: impl Iterator<Item = &'a String>,
    right: impl Iterator<Item = &'a String>,
) -> BTreeSet<&'a String> {
    left.chain(right).collect()
}

fn same_column(left: &SnapshotColumn, right: &SnapshotColumn) -> bool {
    let differ = SchemaDiffer::new();
    differ.normalize_type(&left.data_type) == differ.normalize_type(&right.data_type)
        && left.nullable == right.nullable
        && left.default.as_deref().map(expression_words)
            == right.default.as_deref().map(expression_words)
}

/// An expression's words as the engine means them: [`condition_words`], with quoted cast
/// numbers unquoted and `a BETWEEN x AND y` as `a >= x AND a <= y`.
fn expression_words(text: &str) -> String {
    let words = condition_words(text);
    let words: Vec<&str> = words
        .split(' ')
        .map(|word| {
            word.strip_prefix('\'')
                .and_then(|word| word.strip_suffix('\''))
                .filter(|inner| !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()))
                .unwrap_or(word)
        })
        .collect();
    let mut out: Vec<&str> = Vec::with_capacity(words.len());
    // The `BETWEEN` operand starts after the last `AND`, `OR` or `NOT`.
    let mut operand = 0;
    let mut at = 0;
    while at < words.len() {
        match words.get(at..at + 4) {
            Some(["between", low, "and", high]) => {
                let tested: Vec<&str> = out.drain(operand..).collect();
                out.extend(&tested);
                out.extend([">=", *low, "and"]);
                out.extend(&tested);
                out.extend(["<=", *high]);
                at += 4;
            }
            _ => {
                if matches!(words[at], "and" | "or" | "not") {
                    operand = out.len() + 1;
                }
                out.push(words[at]);
                at += 1;
            }
        }
    }
    out.join(" ")
}

fn column_sql(column: &SnapshotColumn) -> String {
    let nullable = if column.nullable { "NULL" } else { "NOT NULL" };
    let mut written = format!("{} {nullable}", column.data_type);
    if let Some(default) = &column.default {
        written.push_str(" DEFAULT ");
        written.push_str(default);
    }
    written
}

/// An index as SQL writes it after its name: `UNIQUE (a HASH, b ASC) INCLUDE (c) WHERE …`.
fn index_sql(index: &SnapshotIndex) -> String {
    let mut written = String::new();
    if index.constraint {
        written.push_str("CONSTRAINT ");
    }
    if index.unique {
        written.push_str("UNIQUE ");
    }
    written.push_str(&format!("({})", key(&index.key)));
    if !index.include.is_empty() {
        written.push_str(&format!(" INCLUDE ({})", index.include.join(", ")));
    }
    if let Some(condition) = &index.condition {
        written.push_str(&format!(" WHERE {condition}"));
    }
    written
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use fundamental_core::model::sql::ShardKind;

    use super::{SchemaDifference, SchemaItem};
    use crate::migration::KeyPart;
    use crate::migration::generate::{Snapshot, SnapshotColumn, SnapshotIndex, SnapshotTable};

    fn column(name: &str, data_type: &str, nullable: bool) -> SnapshotColumn {
        SnapshotColumn {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable,
            default: None,
        }
    }

    fn split(columns: &[(&str, ShardKind)]) -> Vec<KeyPart> {
        let columns: Vec<(&str, Option<ShardKind>)> =
            columns.iter().map(|(c, s)| (*c, Some(*s))).collect();
        KeyPart::parts(&columns)
    }

    fn articles() -> SnapshotTable {
        SnapshotTable {
            columns: vec![
                column("organisation_id", "UUID", false),
                column("id", "UUID", false),
                column("kind", "TEXT", false),
                column("standfirst", "TEXT", true),
            ],
            key: split(&[("organisation_id", ShardKind::Hash), ("id", ShardKind::Asc)]),
            indexes: BTreeMap::from([(
                "by_kind".to_owned(),
                SnapshotIndex {
                    key: split(&[
                        ("organisation_id", ShardKind::Hash),
                        ("kind", ShardKind::Asc),
                    ]),
                    unique: false,
                    condition: None,
                    include: Vec::new(),
                    constraint: false,
                },
            )]),
            checks: BTreeMap::from([(
                "article_kind".to_owned(),
                "kind IN ('news', 'story')".to_owned(),
            )]),
            readers: BTreeMap::from([("www".to_owned(), "published = true".to_owned())]),
            ..SnapshotTable::default()
        }
    }

    fn snapshot(tables: &[(&str, SnapshotTable)]) -> Snapshot {
        Snapshot {
            tables: tables
                .iter()
                .map(|(name, table)| ((*name).to_owned(), table.clone()))
                .collect(),
            ..Snapshot::default()
        }
    }

    fn the_difference(change: impl FnOnce(&mut SnapshotTable)) -> SchemaDifference {
        let mut found = articles();
        change(&mut found);
        let differences =
            snapshot(&[("article", articles())]).differences(&snapshot(&[("article", found)]));
        assert_eq!(differences.len(), 1, "{differences:#?}");
        differences.into_iter().next().expect("one difference")
    }

    fn placed() -> SnapshotTable {
        SnapshotTable {
            colocated: Some(false),
            tablets: Some(8),
            writers: ["importer".to_owned()].into(),
            ..articles()
        }
    }

    fn between(expected: SnapshotTable, found: SnapshotTable) -> Vec<SchemaDifference> {
        snapshot(&[("article", expected)]).differences(&snapshot(&[("article", found)]))
    }

    #[test]
    fn a_table_placed_otherwise_or_written_by_none_is_named() {
        let differences = between(
            placed(),
            SnapshotTable {
                colocated: Some(true),
                tablets: Some(1),
                ..articles()
            },
        );

        let items: Vec<String> = differences.iter().map(|d| d.item.to_string()).collect();
        assert_eq!(items, ["colocation", "tablets", "writer importer"]);
        assert_eq!(
            differences[0].to_string(),
            "article  colocation  expected apart  found colocated"
        );
    }

    #[test]
    fn what_the_expected_side_leaves_to_the_database_is_not_compared() {
        let found = SnapshotTable {
            colocated: Some(false),
            tablets: Some(3),
            writers: ["app".to_owned()].into(),
            ..articles()
        };

        assert_eq!(between(articles(), found), Vec::new());
        assert_eq!(between(placed(), placed()), Vec::new());
    }

    #[test]
    fn a_snapshot_has_no_differences_from_itself() {
        let committed = snapshot(&[("article", articles())]);
        assert_eq!(committed.differences(&committed), Vec::new());
    }

    #[test]
    fn a_table_either_side_lacks_is_named() {
        let committed = snapshot(&[("article", articles())]);
        let empty = Snapshot::default();
        let missing = committed.differences(&empty);
        assert_eq!(
            missing,
            vec![SchemaDifference {
                table: "article".to_owned(),
                item: SchemaItem::Table,
                expected: Some("present".to_owned()),
                found: None,
            }]
        );
        let unexpected = empty.differences(&committed);
        assert_eq!(unexpected[0].expected, None);
        assert_eq!(unexpected[0].found.as_deref(), Some("present"));
    }

    #[test]
    fn a_column_s_nullability_is_named_with_both_values() {
        let difference = the_difference(|table| table.columns[3].nullable = false);
        assert_eq!(
            difference.to_string(),
            "article  column standfirst  expected TEXT NULL  found TEXT NOT NULL"
        );
    }

    #[test]
    fn a_column_s_type_and_default_are_compared_as_the_engine_means_them() {
        let mut expected = articles();
        expected.columns.push(SnapshotColumn {
            default: Some("''".to_owned()),
            ..column("note", "INT8", false)
        });
        let mut found = expected.clone();
        found.columns[4] = SnapshotColumn {
            default: Some("''::text".to_owned()),
            ..column("note", "BIGINT", false)
        };
        assert_eq!(
            snapshot(&[("article", expected)]).differences(&snapshot(&[("article", found)])),
            Vec::new()
        );
        let difference = the_difference(|table| table.columns[2].data_type = "UUID".to_owned());
        assert_eq!(difference.item, SchemaItem::Column("kind".to_owned()));
    }

    #[test]
    fn a_column_either_side_lacks_is_named() {
        let difference = the_difference(|table| {
            table.columns.remove(3);
        });
        assert_eq!(difference.expected.as_deref(), Some("TEXT NULL"));
        assert_eq!(difference.found, None);
    }

    #[test]
    fn column_order_is_not_compared() {
        let mut found = articles();
        found.columns.swap(2, 3);
        assert_eq!(
            snapshot(&[("article", articles())]).differences(&snapshot(&[("article", found)])),
            Vec::new()
        );
    }

    #[test]
    fn a_key_split_otherwise_is_named_as_sql_writes_it() {
        let difference = the_difference(|table| {
            table.key = split(&[
                ("organisation_id", ShardKind::Hash),
                ("id", ShardKind::Hash),
            ]);
        });
        assert_eq!(
            difference.to_string(),
            "article  key  expected (organisation_id HASH, id ASC)  found ((organisation_id, id) HASH)"
        );
    }

    #[test]
    fn an_index_s_split_is_named_as_sql_writes_it() {
        let difference = the_difference(|table| {
            table.indexes.get_mut("by_kind").expect("by_kind").key = split(&[
                ("organisation_id", ShardKind::Hash),
                ("kind", ShardKind::Hash),
            ]);
        });
        assert_eq!(
            difference.to_string(),
            "article  index by_kind  expected (organisation_id HASH, kind ASC)  found ((organisation_id, kind) HASH)"
        );
    }

    #[test]
    fn an_index_s_uniqueness_condition_and_carried_columns_are_each_compared() {
        let unique = the_difference(|table| {
            table.indexes.get_mut("by_kind").expect("by_kind").unique = true;
        });
        assert_eq!(
            unique.found.as_deref(),
            Some("UNIQUE (organisation_id HASH, kind ASC)")
        );
        let partial = the_difference(|table| {
            table.indexes.get_mut("by_kind").expect("by_kind").condition =
                Some("standfirst IS NULL".to_owned());
        });
        assert_eq!(
            partial.found.as_deref(),
            Some("(organisation_id HASH, kind ASC) WHERE standfirst IS NULL")
        );
        let covering = the_difference(|table| {
            table.indexes.get_mut("by_kind").expect("by_kind").include = vec!["id".to_owned()];
        });
        assert_eq!(
            covering.found.as_deref(),
            Some("(organisation_id HASH, kind ASC) INCLUDE (id)")
        );
    }

    #[test]
    fn an_index_condition_is_compared_as_the_engine_renders_it() {
        let mut expected = articles();
        expected
            .indexes
            .get_mut("by_kind")
            .expect("by_kind")
            .condition = Some("kind IN ('news', 'story')".to_owned());
        let mut found = expected.clone();
        found.indexes.get_mut("by_kind").expect("by_kind").condition =
            Some("(kind = ANY (ARRAY['news'::text, 'story'::text]))".to_owned());
        assert_eq!(
            snapshot(&[("article", expected)]).differences(&snapshot(&[("article", found)])),
            Vec::new()
        );
    }

    #[test]
    fn a_check_is_compared_by_its_expression_as_the_engine_renders_it() {
        let mut found = articles();
        found.checks.insert(
            "article_kind".to_owned(),
            "((kind = ANY (ARRAY['news'::text, 'story'::text])))".to_owned(),
        );
        assert_eq!(
            snapshot(&[("article", articles())]).differences(&snapshot(&[("article", found)])),
            Vec::new()
        );
        let difference = the_difference(|table| {
            table
                .checks
                .insert("article_kind".to_owned(), "kind IN ('news')".to_owned());
        });
        assert_eq!(
            difference.to_string(),
            "article  check article_kind  expected kind IN ('news', 'story')  found kind IN ('news')"
        );
    }

    #[test]
    fn a_range_check_is_the_same_as_the_engine_stores_it() {
        let mut expected = articles();
        expected.checks.insert(
            "article_amount".to_owned(),
            "amount BETWEEN 0 AND 922337203685477".to_owned(),
        );
        let mut found = expected.clone();
        found.checks.insert(
            "article_amount".to_owned(),
            "(((amount >= 0) AND (amount <= '922337203685477'::bigint)))".to_owned(),
        );
        assert_eq!(
            snapshot(&[("article", expected)]).differences(&snapshot(&[("article", found)])),
            Vec::new()
        );
    }

    #[test]
    fn a_range_of_a_function_s_value_beside_another_test_is_the_same_as_stored() {
        let mut expected = articles();
        expected.checks.insert(
            "article_subject".to_owned(),
            "char_length(subject) BETWEEN 1 AND 256 AND subject ~ '^[a-z]+$'".to_owned(),
        );
        let mut found = expected.clone();
        found.checks.insert(
            "article_subject".to_owned(),
            "((((char_length(subject) >= 1) AND (char_length(subject) <= 256)) AND \
             (subject ~ '^[a-z]+$'::text)))"
                .to_owned(),
        );
        assert_eq!(
            snapshot(&[("article", expected)]).differences(&snapshot(&[("article", found)])),
            Vec::new()
        );
    }

    #[test]
    fn a_reader_s_rows_are_named() {
        let difference = the_difference(|table| {
            table.readers.clear();
        });
        assert_eq!(
            difference.to_string(),
            "article  reader www  expected published = true  found none"
        );
    }
}
