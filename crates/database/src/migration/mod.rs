//! Schema migrations generated from the models, and the vocabulary for reasoning about them.
//!
//! The model is the schema: [`generate`] writes the next forward-only file from the difference
//! between the models and the committed `migrations/schema.json`, and refuses whatever a model
//! does not declare the meaning of (a drop, a retype, a reshaped index). Each [`lint`] has a
//! stable name and code and a level set in `fun.toml`.

use chrono::{DateTime, Utc};

mod changes;
mod config;
mod content_hash;
mod database;
pub mod ddl;
mod diff;
mod dollar_quote;
mod drift;
mod embedded_migration;
mod embedded_migrations;
mod error;
pub mod generate;
mod guard;
mod introspect;
mod ledger;
pub mod lint;
pub mod render;
mod result;
mod revision;
mod safety;
mod schema_source;
mod sql;
mod table;

pub use config::MigrationConfig;
pub use content_hash::content_hash;
pub use database::MigrationDatabase;
pub use drift::SchemaDrift;
pub use embedded_migration::EmbeddedMigration;
pub use embedded_migrations::{EmbeddedMigrations, Generated};
pub use error::MigrationError;
pub use guard::{Exemption, GuardError, MigrationGuard, Retirements};
pub use introspect::SchemaIntrospector;
pub use ledger::{
    CatalogProof, FailedStep, LedgerEntry, LedgerError, LedgerState, MIGRATIONS_TABLE,
    MigrationLedger, RESOLUTIONS_TABLE, Resolution, Resolve, Resolved, STEPS_TABLE, StepRecord,
    StepReplay, StepState, is_ledger_table,
};
pub use lint::{Level, Lint, Lints, LintsError, lints};
pub use result::MigrationResult;
pub use revision::Revision;
pub use safety::SafetyTier;
pub use schema_source::ModelSchema;
pub use sql::split_statements;
pub(crate) use sql::statement_spans;

pub use changes::*;
pub use diff::*;
pub use table::*;

/// A migration that transforms database schema.
#[derive(Debug, Clone)]
pub struct Migration {
    /// Unique migration name (usually timestamp + description).
    pub name: String,
    /// When this migration was generated.
    pub generated_at: DateTime<Utc>,
    /// The database type this migration targets.
    pub database: MigrationDatabase,
    /// The SQL/CQL statements to apply the migration.
    pub up_statements: Vec<String>,
    /// The SQL/CQL statements to rollback the migration.
    pub down_statements: Vec<String>,
    /// The schema changes in this migration.
    pub changes: Vec<SchemaChange>,
    /// Safety tier for this migration.
    pub safety_tier: SafetyTier,
    /// Whether this migration has been applied.
    pub applied: bool,
    /// When this migration was applied (if applicable).
    pub applied_at: Option<DateTime<Utc>>,
}

impl Migration {
    /// Creates a new migration.
    pub fn new(
        name: impl Into<String>,
        database_type: MigrationDatabase,
        changes: Vec<SchemaChange>,
    ) -> Self {
        let safety_tier = SafetyTier::from_changes(&changes);
        Self {
            name: name.into(),
            generated_at: Utc::now(),
            database: database_type,
            up_statements: Vec::new(),
            down_statements: Vec::new(),
            changes,
            safety_tier,
            applied: false,
            applied_at: None,
        }
    }

    /// Generates a migration name from timestamp and description.
    pub fn generate_name(description: &str) -> String {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let slug = description
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>();
        format!("{}_{}", timestamp, slug)
    }

    /// Returns true if this migration is safe to auto-apply.
    pub fn is_safe(&self) -> bool {
        self.safety_tier == SafetyTier::Safe
    }

    /// Returns true if this migration requires review.
    pub fn requires_review(&self) -> bool {
        self.safety_tier == SafetyTier::Review
    }

    /// Returns true if this migration is destructive.
    pub fn is_destructive(&self) -> bool {
        self.safety_tier == SafetyTier::Destructive
    }

    /// Returns the file extension for this migration.
    pub fn file_extension(&self) -> &'static str {
        match self.database {
            MigrationDatabase::SqlDb => "sql",
            MigrationDatabase::CqlDb => "cql",
            MigrationDatabase::OfflineDb => "sql",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safety_tier_ordering() {
        assert!(SafetyTier::Safe < SafetyTier::Review);
        assert!(SafetyTier::Review < SafetyTier::Destructive);
    }

    #[test]
    fn test_safety_tier_from_changes() {
        let changes = vec![SchemaChange::CreateTable(CreateTableChange {
            table_name: "test".into(),
            columns: vec![],
            primary_key: vec![],
            indexes: vec![],
            foreign_keys: vec![],
            check_constraints: vec![],
            partition_key: None,
            clustering_key: None,
            table_options: None,
        })];
        assert_eq!(SafetyTier::from_changes(&changes), SafetyTier::Safe);

        let changes = vec![SchemaChange::DropTable(DropTableChange {
            table_name: "test".into(),
            cascade: false,
        })];
        assert_eq!(SafetyTier::from_changes(&changes), SafetyTier::Destructive);
    }

    #[test]
    fn test_migration_name_generation() {
        let name = Migration::generate_name("add user avatar");
        assert!(name.contains("add_user_avatar"));
        assert!(name.len() > 15); // Timestamp + description
    }
}
