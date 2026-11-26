//! [`KeyPart`]: one part of a table's or an index's key, and how it is split.

use fundamental_core::model::sql::ShardKind;

/// One part of a key: one or more columns and how their values are spread. Only the first part
/// may be hashed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyPart {
    /// Its columns, in order.
    pub columns: Vec<String>,
    /// How they are split.
    pub split: ShardKind,
    /// Where an ordered part keeps its empty values, when not where the engine keeps them by
    /// default (last ascending, first descending).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nulls: Option<Nulls>,
}

/// Where an ordered key part keeps its empty values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Nulls {
    /// Before every value.
    First,
    /// After every value.
    Last,
}

impl Nulls {
    /// Reads `FIRST` or `LAST`, in any case.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word.to_ascii_uppercase().as_str() {
            "FIRST" => Some(Self::First),
            "LAST" => Some(Self::Last),
            _ => None,
        }
    }

    /// The words a migration spells it with.
    #[must_use]
    pub const fn as_sql(self) -> &'static str {
        match self {
            Self::First => "NULLS FIRST",
            Self::Last => "NULLS LAST",
        }
    }
}

impl KeyPart {
    /// The parts of a key whose columns, in order, say how each is split; a column that says
    /// nothing is hashed when it leads and ascending after.
    #[must_use]
    pub fn parts(columns: &[(&str, Option<ShardKind>)]) -> Vec<Self> {
        let ordered: Vec<(&str, Option<ShardKind>, Option<Nulls>)> = columns
            .iter()
            .map(|(column, split)| (*column, *split, None))
            .collect();
        Self::ordered_parts(&ordered)
    }

    /// [`parts`](Self::parts), each column also saying where it keeps its empty values.
    #[must_use]
    pub fn ordered_parts(columns: &[(&str, Option<ShardKind>, Option<Nulls>)]) -> Vec<Self> {
        let mut parts: Vec<Self> = Vec::new();
        for (index, (column, split, nulls)) in columns.iter().enumerate() {
            let split = split.unwrap_or(if index == 0 {
                ShardKind::Hash
            } else {
                ShardKind::Asc
            });
            match parts.last_mut() {
                Some(last) if split == ShardKind::Hash && last.split == ShardKind::Hash => {
                    last.columns.push((*column).to_owned());
                }
                _ => parts.push(Self {
                    columns: vec![(*column).to_owned()],
                    split,
                    nulls: *nulls,
                }),
            }
        }
        parts
    }
}
