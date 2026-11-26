//! [`DdlStatement`]: one migration statement, in the vocabulary the guards need.

use std::collections::BTreeMap;

use super::{DdlColumn, KeyDeclaration};

/// A recognised migration statement.
///
/// Not a SQL AST: it carries what the guards ask and calls everything else
/// [`Other`](Self::Other).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdlStatement {
    /// `CREATE TABLE …`
    CreateTable {
        /// The table's name, unqualified.
        table: String,
        /// The table-level `PRIMARY KEY (…)`, when the statement declares one.
        primary_key: Option<KeyDeclaration>,
        /// Columns carrying an inline `PRIMARY KEY`, which the engine's grammar gives no room to
        /// shard.
        inline_primary_key_columns: Vec<String>,
        /// The column lists of `UNIQUE` constraints, inline or table-level.
        ///
        /// Each becomes a unique index, and a `UNIQUE` constraint cannot carry a sharding token, so
        /// the guard asks for a separate `CREATE UNIQUE INDEX`.
        unique_constraints: Vec<Vec<String>>,
        /// Upper bounds a `CHECK (col IN (…))` puts on a column's distinct values.
        value_bounds: BTreeMap<String, u64>,
        /// Its columns, in order, each with its type and default.
        columns: Vec<DdlColumn>,
        /// What `WITH (COLOCATION = …)` says, when it says it.
        colocated: Option<bool>,
        /// What `SPLIT INTO <n> TABLETS` says, when it says it.
        tablets: Option<u32>,
    },

    /// `CREATE [UNIQUE] INDEX …`
    CreateIndex {
        /// The index's name.
        name: String,
        /// The table it covers.
        table: String,
        /// Its key list.
        key: KeyDeclaration,
        /// Whether it is built while the table goes on taking writes: `false` only for
        /// `CREATE INDEX NONCONCURRENTLY`.
        online: bool,
    },

    /// A statement that removes data, a schema object or a name running code reads: `DROP …`,
    /// `TRUNCATE …`, and `ALTER TABLE … DROP COLUMN`, `DROP CONSTRAINT` or `RENAME COLUMN`.
    Destructive {
        /// The verb, uppercased, such as `DROP TABLE` or `RENAME COLUMN`.
        verb: String,
        /// What it names: a table or index unqualified (`widgets`), and a column or constraint
        /// as `<table>.<name>` (`widgets.legacy`), the name before for a rename.
        object: String,
    },

    /// `ALTER TABLE … ADD COLUMN …`, whose safety turns on nullability and a default.
    AddColumn {
        /// The table.
        table: String,
        /// The column being added.
        column: String,
        /// Whether the new column admits `NULL`.
        nullable: bool,
        /// Whether the new column carries a `DEFAULT`.
        has_default: bool,
        /// Its type, uppercased as written.
        data_type: String,
        /// Its default as written, when it has one.
        default: Option<String>,
    },

    /// `ALTER TABLE … ALTER COLUMN … TYPE …`: the column retyped.
    AlterColumnType {
        /// The table.
        table: String,
        /// The column.
        column: String,
        /// The type it becomes, uppercased as written.
        to: String,
    },

    /// `ALTER TABLE … ADD [CONSTRAINT …] PRIMARY KEY (…)`.
    AddPrimaryKey {
        /// The table.
        table: String,
        /// The key, which declares its sharding as a `CREATE TABLE`'s must.
        key: KeyDeclaration,
    },
    /// `ALTER TABLE … ALTER COLUMN … SET NOT NULL`.
    SetNotNull {
        /// The table.
        table: String,
        /// The column.
        column: String,
    },

    /// A statement that moves rows: `UPDATE`, `DELETE`, `INSERT … SELECT` and `COPY`. An
    /// `INSERT … VALUES` of fixed rows is a seed, and is [`Other`](Self::Other).
    Data {
        /// The verb, uppercased: `UPDATE`, `DELETE`, `INSERT`, `COPY`.
        verb: String,
        /// The table whose rows it writes.
        table: String,
        /// Whether its rows come from a query, as an `INSERT … SELECT`'s do.
        from_select: bool,
    },

    /// A role made: `CREATE ROLE …`, alone or inside a `DO` block.
    ///
    /// Safe to run again for a login without `CREATEROLE` only when a `pg_roles` lookup guards
    /// it; a duplicate handler is not enough.
    CreateRole {
        /// The role, as written.
        role: String,
        /// Whether the statement is made only when `pg_roles` lacks the role: `IF NOT EXISTS
        /// (SELECT FROM pg_roles WHERE rolname = '<role>') THEN CREATE ROLE <role>`.
        looked_up: bool,
    },

    /// Anything else, which the guards do not judge and so never refuse.
    Other {
        /// The statement's leading words, for a log line that names what was skipped.
        lead: String,
    },
}

impl DdlStatement {
    /// The key declarations this statement introduces, each with the object it belongs to.
    #[must_use]
    pub fn key_declarations(&self) -> Vec<(&str, &KeyDeclaration)> {
        match self {
            Self::CreateTable {
                table, primary_key, ..
            } => primary_key
                .as_ref()
                .map(|key| vec![(table.as_str(), key)])
                .unwrap_or_default(),
            Self::CreateIndex { name, key, .. } => vec![(name.as_str(), key)],
            Self::AddPrimaryKey { table, key } => vec![(table.as_str(), key)],
            Self::Destructive { .. }
            | Self::AddColumn { .. }
            | Self::AlterColumnType { .. }
            | Self::SetNotNull { .. }
            | Self::Data { .. }
            | Self::CreateRole { .. }
            | Self::Other { .. } => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::DdlStatement;
    use crate::migration::ddl::{KeyDeclaration, KeyElement, Sharding};

    fn key() -> KeyDeclaration {
        KeyDeclaration::new(vec![KeyElement::new(
            vec!["id".to_owned()],
            Some(Sharding::Hash),
        )])
    }

    #[test]
    fn a_table_and_an_index_both_offer_their_key() {
        let table = DdlStatement::CreateTable {
            table: "widgets".to_owned(),
            primary_key: Some(key()),
            inline_primary_key_columns: Vec::new(),
            unique_constraints: Vec::new(),
            value_bounds: BTreeMap::new(),
            columns: Vec::new(),
            colocated: None,
            tablets: None,
        };
        let index = DdlStatement::CreateIndex {
            name: "widgets_by_owner".to_owned(),
            table: "widgets".to_owned(),
            key: key(),
            online: true,
        };

        assert_eq!(table.key_declarations()[0].0, "widgets");
        assert_eq!(index.key_declarations()[0].0, "widgets_by_owner");
    }

    #[test]
    fn a_statement_with_no_key_offers_none() {
        let dropped = DdlStatement::Destructive {
            verb: "DROP TABLE".to_owned(),
            object: "widgets".to_owned(),
        };
        let other = DdlStatement::Other {
            lead: "INSERT INTO".to_owned(),
        };

        assert!(dropped.key_declarations().is_empty());
        assert!(other.key_declarations().is_empty());
    }
}
