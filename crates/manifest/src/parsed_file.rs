//! One manifest file parsed into the values it defines.

use std::str::FromStr;

use toml::Value;
use toml_edit::{Document, Item, Table, TableLike};

use crate::leaf::Leaf;
use crate::source_file::SourceFile;
use crate::{KeyPath, ManifestFinding, Origin};

/// Every value `source` defines, with where each is written, or the finding that it is not
/// TOML.
pub(crate) fn parse_leaves(source: &SourceFile) -> Result<Vec<Leaf>, ManifestFinding> {
    let document =
        Document::parse(source.text.as_str()).map_err(|error| ManifestFinding::Unparseable {
            at: Origin::at_offset(
                &source.file,
                &source.text,
                error.span().map_or(0, |span| span.start),
            )
            .to_string(),
            cause: error.message().to_owned(),
        })?;
    let mut leaves = Vec::new();
    collect_table(
        source,
        document.as_table(),
        &KeyPath::default(),
        &mut leaves,
    )?;
    Ok(leaves)
}

fn collect_table(
    source: &SourceFile,
    table: &dyn TableLike,
    path: &KeyPath,
    leaves: &mut Vec<Leaf>,
) -> Result<(), ManifestFinding> {
    for (name, item) in table.iter() {
        let key = path.child(name);
        let offset = table
            .get_key_value(name)
            .and_then(|(key, _)| key.span())
            .or_else(|| item.span())
            .map_or(0, |span| span.start);
        match item {
            Item::None => {}
            Item::Table(child) if !child.is_empty() => collect_table(source, child, &key, leaves)?,
            Item::Table(child) if child.is_implicit() => {}
            Item::Value(toml_edit::Value::InlineTable(child)) if !child.is_empty() => {
                collect_table(source, child, &key, leaves)?;
            }
            other => {
                let origin_offset = match other {
                    Item::Table(child) => child.span().map_or(offset, |span| span.start),
                    Item::ArrayOfTables(array) => array
                        .iter()
                        .next()
                        .and_then(Table::span)
                        .map_or(offset, |span| span.start),
                    _ => offset,
                };
                let origin = Origin::at_offset(&source.file, &source.text, origin_offset);
                let value = to_value(other).map_err(|cause| ManifestFinding::Unparseable {
                    at: origin.to_string(),
                    cause,
                })?;
                leaves.push(Leaf { key, value, origin });
            }
        }
    }
    Ok(())
}

/// The plain value of `item`.
pub(crate) fn to_value(item: &Item) -> Result<Value, String> {
    match item {
        Item::None => Ok(Value::Table(toml::Table::new())),
        Item::Value(value) => from_edit(value),
        Item::Table(table) => table_value(table),
        Item::ArrayOfTables(array) => array
            .iter()
            .map(|table| table_value(table))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
    }
}

fn table_value(table: &dyn TableLike) -> Result<Value, String> {
    let mut map = toml::Table::new();
    for (name, item) in table.iter() {
        if !item.is_none() {
            map.insert(name.to_owned(), to_value(item)?);
        }
    }
    Ok(Value::Table(map))
}

fn from_edit(value: &toml_edit::Value) -> Result<Value, String> {
    Ok(match value {
        toml_edit::Value::String(text) => Value::String(text.value().clone()),
        toml_edit::Value::Integer(number) => Value::Integer(*number.value()),
        toml_edit::Value::Float(number) => Value::Float(*number.value()),
        toml_edit::Value::Boolean(flag) => Value::Boolean(*flag.value()),
        toml_edit::Value::Datetime(moment) => {
            let written = moment.value().to_string();
            Value::Datetime(
                toml::value::Datetime::from_str(&written).map_err(|error| {
                    format!("the date-time {written} does not read back: {error}")
                })?,
            )
        }
        toml_edit::Value::Array(array) => {
            Value::Array(array.iter().map(from_edit).collect::<Result<_, _>>()?)
        }
        toml_edit::Value::InlineTable(table) => table_value(table)?,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn leaves_of(text: &str) -> Vec<(String, Value, String)> {
        let source = SourceFile {
            file: PathBuf::from("f.toml"),
            text: text.to_owned(),
        };
        parse_leaves(&source)
            .map(|leaves| {
                leaves
                    .into_iter()
                    .map(|leaf| (leaf.key.to_string(), leaf.value, leaf.origin.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_table_is_its_values_and_an_empty_table_is_a_value() {
        let leaves = leaves_of("[a]\nx = 1\ny = { z = \"w\" }\n\n[b]\n");
        let keys: Vec<_> = leaves
            .iter()
            .map(|(key, _, at)| (key.as_str(), at.as_str()))
            .collect();
        assert_eq!(
            keys,
            [
                ("a.x", "f.toml:2:1"),
                ("a.y.z", "f.toml:3:7"),
                ("b", "f.toml:5:1")
            ]
        );
    }

    #[test]
    fn dotted_keys_and_implicit_tables_are_walked() {
        let leaves = leaves_of("[c.d]\ne.f = true\n\"g.h\" = 2\n");
        let keys: Vec<_> = leaves.iter().map(|(key, _, _)| key.as_str()).collect();
        assert_eq!(keys, ["c.d.e.f", "c.d.\"g.h\""]);
    }

    #[test]
    fn an_array_of_tables_is_one_value_at_its_first_header() {
        let leaves = leaves_of("x = 1\n[[r]]\nid = \"1\"\n[[r.q]]\nt = 2\n[[r]]\nid = \"2\"\n");
        assert_eq!(leaves.len(), 2);
        let (key, value, at) = &leaves[1];
        assert_eq!((key.as_str(), at.as_str()), ("r", "f.toml:2:1"));
        let ids: Vec<_> = value
            .as_array()
            .map(|tables| {
                tables
                    .iter()
                    .filter_map(|t| t.get("id")?.as_str())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(ids, ["1", "2"]);
        assert_eq!(
            value
                .as_array()
                .and_then(|t| t[0].get("q"))
                .map(Value::is_array),
            Some(true)
        );
    }

    #[test]
    fn names_the_line_of_a_parse_error() {
        let source = SourceFile {
            file: PathBuf::from("f.toml"),
            text: "[a]\nx = \n".to_owned(),
        };
        let refused = parse_leaves(&source);
        assert!(
            matches!(&refused, Err(ManifestFinding::Unparseable { at, .. }) if at.starts_with("f.toml:2:")),
            "{refused:?}"
        );
    }
}
