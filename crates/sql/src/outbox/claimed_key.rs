use fundamental_core::model::SqlModel;
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, TypeInfo};

use crate::SqlError;
use crate::ident::validate_identifier;

/// The key column types whose text copy casts back to exactly the stored value.
const ADDRESSABLE: [&str; 7] = [
    "UUID",
    "TEXT",
    "VARCHAR",
    "INT4",
    "INT8",
    "TIMESTAMPTZ",
    "BYTEA",
];

/// A claimed row's whole key: each key column, its database type, and its value as text; not `Debug`, as a key can identify a person.
pub(crate) struct ClaimedKey {
    parts: Vec<(&'static str, &'static str, String)>,
}

impl ClaimedKey {
    #[cfg(test)]
    pub(crate) fn from_parts(parts: Vec<(&'static str, &'static str, String)>) -> Self {
        Self { parts }
    }

    pub(crate) fn alias(index: usize) -> String {
        format!("__outbox_key_{index}")
    }

    /// Reads the key copy from `row`, typing each part by its key column's own database type.
    pub(crate) fn read<M: SqlModel>(row: &PgRow) -> Result<Self, SqlError> {
        let mut parts = Vec::with_capacity(M::KEY_COLUMNS.len());
        for (index, column) in M::KEY_COLUMNS.iter().enumerate() {
            validate_identifier(column, false)?;
            let type_name = row
                .columns()
                .iter()
                .find(|candidate| candidate.name() == *column)
                .map_or("", |found| found.type_info().name());
            let Some(cast) = ADDRESSABLE.iter().find(|allowed| **allowed == type_name) else {
                return Err(SqlError::UnaddressableKey {
                    table: M::TABLE,
                    column,
                    type_name: type_name.to_owned(),
                });
            };
            let text: String = row.try_get(Self::alias(index).as_str())?;
            parts.push((*column, *cast, text));
        }
        Ok(Self { parts })
    }

    /// The key as the parts a set-aside seal covers: each key column's text copy, which any reader can reproduce.
    pub(crate) fn seal_parts(&self) -> Vec<(&'static str, Option<Vec<u8>>)> {
        self.parts
            .iter()
            .map(|(column, _, text)| (*column, Some(text.as_bytes().to_vec())))
            .collect()
    }

    pub(crate) fn predicate(&self, first: usize) -> (String, Vec<&str>) {
        let clauses: Vec<String> = self
            .parts
            .iter()
            .enumerate()
            .map(|(offset, (column, cast, _))| format!("{column} = ${}::{cast}", first + offset))
            .collect();
        let values = self
            .parts
            .iter()
            .map(|(_, _, text)| text.as_str())
            .collect();
        (clauses.join(" AND "), values)
    }
}

#[cfg(test)]
mod tests {
    use super::ClaimedKey;

    #[test]
    fn a_two_column_key_addresses_each_column_by_its_own_type() {
        let key = ClaimedKey {
            parts: vec![
                ("zone", "TEXT", "eu".to_owned()),
                (
                    "id",
                    "UUID",
                    "7f1c0000-0000-0000-0000-000000000000".to_owned(),
                ),
            ],
        };
        let (predicate, values) = key.predicate(3);
        assert_eq!(predicate, "zone = $3::TEXT AND id = $4::UUID");
        assert_eq!(values, ["eu", "7f1c0000-0000-0000-0000-000000000000"]);
    }

    #[test]
    fn every_key_copy_has_its_own_alias() {
        assert_eq!(ClaimedKey::alias(0), "__outbox_key_0");
        assert_ne!(ClaimedKey::alias(0), ClaimedKey::alias(1));
    }
}
