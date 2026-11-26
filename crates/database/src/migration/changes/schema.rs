//! The [`SchemaChange`] enum over every individual change type.

use super::*;
use crate::migration::SafetyTier;

/// A single schema change.
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaChange {
    /// Create a new table.
    CreateTable(CreateTableChange),
    /// Drop an existing table.
    DropTable(DropTableChange),
    /// Rename a table.
    RenameTable(RenameTableChange),
    /// Add a column to a table.
    AddColumn(AddColumnChange),
    /// Drop a column from a table.
    DropColumn(DropColumnChange),
    /// Alter a column (type, nullability, default).
    AlterColumn(AlterColumnChange),
    /// Rename a column.
    RenameColumn(RenameColumnChange),
    /// Reorder columns in a table.
    ReorderColumns(ReorderColumnsChange),
    /// Create an index.
    CreateIndex(CreateIndexChange),
    /// Drop an index.
    DropIndex(DropIndexChange),
    /// Add a foreign key constraint.
    AddForeignKey(AddForeignKeyChange),
    /// Drop a foreign key constraint.
    DropForeignKey(DropForeignKeyChange),
    /// Add a check constraint.
    AddCheckConstraint(AddCheckConstraintChange),
    /// Drop a check constraint.
    DropCheckConstraint(DropCheckConstraintChange),
    /// YCQL: Create materialized view.
    CreateMaterializedView(CreateMaterializedViewChange),
    /// YCQL: Drop materialized view.
    DropMaterializedView(DropMaterializedViewChange),
    /// YCQL: Change partition/clustering key (requires new table).
    ChangeKeyStructure(ChangeKeyStructureChange),
    /// SQL: change a table's primary key or its split (requires new table).
    ChangeKey(ChangeKeyChange),
    /// SQL: a role reads a table's rows otherwise than its model says.
    ChangeReader(ChangeReaderChange),
    /// SQL: a role the model lets write a table does not.
    ChangeWriter(ChangeWriterChange),
    /// SQL: a table is placed otherwise than its model says (requires table recreation).
    ChangePlacement(ChangePlacementChange),
}

impl SchemaChange {
    /// Returns the safety tier for this change.
    pub fn safety_tier(&self) -> SafetyTier {
        match self {
            Self::CreateTable(_) => SafetyTier::Safe,
            Self::CreateIndex(c) if !c.unique => SafetyTier::Safe,
            Self::CreateMaterializedView(_) => SafetyTier::Safe,
            Self::AddColumn(c) if c.column.nullable || c.column.default.is_some() => {
                SafetyTier::Safe
            }

            Self::AddColumn(_) => SafetyTier::Review, // Non-null without default
            Self::CreateIndex(_) => SafetyTier::Review, // Unique index might fail
            Self::AddForeignKey(_) => SafetyTier::Review, // Might fail if data violates
            Self::AddCheckConstraint(_) => SafetyTier::Review, // Might fail if data violates
            Self::DropIndex(_) => SafetyTier::Review,
            Self::DropForeignKey(_) => SafetyTier::Review,
            Self::DropCheckConstraint(_) => SafetyTier::Review,

            Self::DropTable(_) => SafetyTier::Destructive,
            Self::DropColumn(_) => SafetyTier::Destructive,
            Self::RenameTable(_) => SafetyTier::Destructive,
            Self::RenameColumn(_) => SafetyTier::Destructive,
            Self::ReorderColumns(_) => SafetyTier::Review,
            Self::AlterColumn(_) => SafetyTier::Destructive,
            Self::DropMaterializedView(_) => SafetyTier::Destructive,
            Self::ChangeKeyStructure(_) => SafetyTier::Destructive,
            Self::ChangeKey(_) => SafetyTier::Destructive,
            Self::ChangeReader(_) | Self::ChangeWriter(_) => SafetyTier::Review,
            Self::ChangePlacement(_) => SafetyTier::Destructive,
        }
    }

    /// Returns a human-readable description.
    pub fn description(&self) -> String {
        match self {
            Self::CreateTable(c) => format!("Create table '{}'", c.table_name),
            Self::DropTable(c) => format!("Drop table '{}'", c.table_name),
            Self::RenameTable(c) => format!("Rename table '{}' to '{}'", c.old_name, c.new_name),
            Self::AddColumn(c) => format!("Add column '{}' to '{}'", c.column.name, c.table_name),
            Self::DropColumn(c) => {
                format!("Drop column '{}' from '{}'", c.column_name, c.table_name)
            }
            Self::AlterColumn(c) => {
                format!("Alter column '{}' in '{}'", c.column_name, c.table_name)
            }
            Self::RenameColumn(c) => format!(
                "Rename column '{}' to '{}' in '{}'",
                c.old_name, c.new_name, c.table_name
            ),
            Self::ReorderColumns(c) => format!("Reorder columns in '{}'", c.table_name),
            Self::CreateIndex(c) => {
                format!("Create index '{}' on '{}'", c.index_name, c.table_name)
            }
            Self::DropIndex(c) => format!("Drop index '{}' from '{}'", c.index_name, c.table_name),
            Self::AddForeignKey(c) => format!(
                "Add foreign key '{}' to '{}'",
                c.constraint_name, c.table_name
            ),
            Self::DropForeignKey(c) => format!(
                "Drop foreign key '{}' from '{}'",
                c.constraint_name, c.table_name
            ),
            Self::AddCheckConstraint(c) => format!(
                "Add check constraint '{}' to '{}'",
                c.constraint_name, c.table_name
            ),
            Self::DropCheckConstraint(c) => format!(
                "Drop check constraint '{}' ({}) from '{}'",
                c.constraint_name, c.expression, c.table_name
            ),
            Self::CreateMaterializedView(c) => {
                format!("Create materialized view '{}'", c.view_name)
            }
            Self::DropMaterializedView(c) => format!("Drop materialized view '{}'", c.view_name),
            Self::ChangeKeyStructure(c) => format!(
                "Change key structure for '{}' (requires table recreation)",
                c.table_name
            ),
            Self::ChangeKey(c) => format!(
                "Change the key of '{}' from ({}) to ({}) (requires table recreation)",
                c.table_name,
                crate::migration::render::key(&c.old_key),
                crate::migration::render::key(&c.new_key)
            ),
            Self::ChangeReader(c) => c.description(),
            Self::ChangeWriter(c) => c.description(),
            Self::ChangePlacement(c) => c.description(),
        }
    }
}
