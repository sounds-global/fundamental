//! Migration manager: what is on disk, and how the live schema compares.

use fundamental_db::migration::{
    DatabaseSchema, DiffConfig, Migration, MigrationConfig, MigrationDatabase, MigrationError,
    MigrationResult, SafetyTier, SchemaChange, SchemaDiffer, SchemaDrift,
};

use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::field::Empty;
use tracing::{Span, debug, instrument, warn};

use crate::migration_path::up_file;

/// Migration manager for generating and applying migrations.
#[derive(Debug)]
pub struct MigrationManager {
    config: MigrationConfig,
}

impl MigrationManager {
    /// Creates a new migration manager.
    pub fn new(config: MigrationConfig) -> Self {
        Self { config }
    }

    /// Creates a manager with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(MigrationConfig::default())
    }

    /// Returns the configuration.
    pub fn config(&self) -> &MigrationConfig {
        &self.config
    }

    /// Loads a migration from disk.
    ///
    /// # Errors
    /// Returns [`MigrationError::UnsafeName`] when `name` is not a single ordinary path
    /// component, [`MigrationError::NotFound`] when no such migration exists, or an IO or
    /// parse error.
    #[instrument(skip(self), fields(migration = %name, database = ?database, path = Empty), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn load_migration(
        &self,
        name: &str,
        database: MigrationDatabase,
    ) -> MigrationResult<Migration> {
        let dir = self.migration_directory(database);
        let ext = match database {
            MigrationDatabase::SqlDb => "sql",
            MigrationDatabase::CqlDb => "cql",
            MigrationDatabase::OfflineDb => "sql",
        };

        let path = up_file(&dir, name, ext)?;
        Span::current().record("path", path.display().to_string());

        if !path.exists() {
            return Err(MigrationError::NotFound(name.to_string()));
        }

        let content = fs::read_to_string(&path)?;
        self.parse_migration_file(name, database, &content, &path)
    }

    /// Lists all migrations for a database type.
    #[instrument(skip(self), fields(database = ?database, count = Empty), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn list_migrations(&self, database: MigrationDatabase) -> MigrationResult<Vec<String>> {
        let dir = self.migration_directory(database);

        if !dir.exists() {
            return Ok(vec![]);
        }

        let ext = match database {
            MigrationDatabase::SqlDb => "sql",
            MigrationDatabase::CqlDb => "cql",
            MigrationDatabase::OfflineDb => "sql",
        };

        let mut migrations = Vec::new();

        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                // Only the `.up.<ext>` files name a migration; `.down.<ext>` is
                // its rollback companion.
                if let Some(stem) = file_name.strip_suffix(&format!(".up.{}", ext)) {
                    migrations.push(stem.to_string());
                }
            }
        }

        migrations.sort();
        Span::current().record("count", migrations.len());
        Ok(migrations)
    }

    /// Returns the directory for migrations of a specific database type.
    pub fn migration_directory(&self, database_type: MigrationDatabase) -> PathBuf {
        self.config
            .migrations_dir
            .join(database_type.directory_name())
    }

    /// Verifies that the database schema matches the expected schema.
    #[instrument(skip(self, expected, actual), fields(drifts = Empty), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn verify_schema(
        &self,
        expected: &DatabaseSchema,
        actual: &DatabaseSchema,
    ) -> MigrationResult<Vec<SchemaDrift>> {
        self.verify_schema_with_config(expected, actual, DiffConfig::default())
    }

    /// Verifies a schema using a specific [`DiffConfig`] (for YCQL, column
    /// order is ignored, since CQL does not preserve it).
    #[instrument(skip(self, expected, actual, config), fields(drifts = Empty), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn verify_schema_with_config(
        &self,
        expected: &DatabaseSchema,
        actual: &DatabaseSchema,
        config: DiffConfig,
    ) -> MigrationResult<Vec<SchemaDrift>> {
        let differ = SchemaDiffer::with_config(config);
        let changes = differ.diff(expected, actual);

        if changes.is_empty() {
            debug!("schema verification found no drift");
            return Ok(vec![]);
        }

        let drifts: Vec<SchemaDrift> = changes
            .iter()
            .map(|change| SchemaDrift {
                table_name: self.extract_table_name(change),
                description: change.description(),
                expected: "from model".to_string(),
                actual: "from database".to_string(),
            })
            .collect();

        Span::current().record("drifts", drifts.len());
        warn!(
            drifts = drifts.len(),
            "schema drift detected between model and database"
        );
        Ok(drifts)
    }

    /// Checks if a migration is safe to apply without --force.
    #[instrument(
        skip(self, migration),
        fields(
            migration = %migration.name,
            environment = %self.config.environment,
            tier = ?migration.safety_tier,
            protected = self.config.is_protected
        ),
        err(Debug)
    )]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn check_safety(&self, migration: &Migration) -> MigrationResult<()> {
        if self.config.is_protected && migration.safety_tier != SafetyTier::Safe {
            warn!(
                reason = "protected_environment",
                "migration blocked by safety check"
            );
            return Err(MigrationError::SafetyCheck {
                message: format!(
                    "Environment '{}' is protected. Migration '{}' has {} changes",
                    self.config.environment, migration.name, migration.safety_tier
                ),
                changes: migration.changes.clone(),
            });
        }

        if migration.safety_tier == SafetyTier::Destructive {
            warn!(
                reason = "destructive_changes",
                "migration blocked by safety check"
            );
            return Err(MigrationError::SafetyCheck {
                message: format!(
                    "Migration '{}' contains destructive changes",
                    migration.name
                ),
                changes: migration
                    .changes
                    .iter()
                    .filter(|c| c.safety_tier() == SafetyTier::Destructive)
                    .cloned()
                    .collect(),
            });
        }

        debug!("migration passed safety check");
        Ok(())
    }

    /// Filters changes by safety tier.
    pub fn filter_safe_changes(&self, changes: &[SchemaChange]) -> Vec<SchemaChange> {
        changes
            .iter()
            .filter(|c| c.safety_tier() == SafetyTier::Safe)
            .cloned()
            .collect()
    }

    fn parse_migration_file(
        &self,
        name: &str,
        database: MigrationDatabase,
        content: &str,
        _path: &Path,
    ) -> MigrationResult<Migration> {
        let statements: Vec<String> = content
            .lines()
            .filter(|line| !line.trim().starts_with("--") && !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .map(|s| format!("{};", s.trim()))
            .collect();

        Ok(Migration {
            name: name.to_string(),
            generated_at: Utc::now(), // Would parse from header
            database,
            up_statements: statements,
            down_statements: vec![],
            changes: vec![], // Would need to reverse-engineer from statements
            safety_tier: SafetyTier::Review, // Default to review for loaded migrations
            applied: false,
            applied_at: None,
        })
    }

    fn extract_table_name(&self, change: &SchemaChange) -> String {
        match change {
            SchemaChange::CreateTable(c) => c.table_name.clone(),
            SchemaChange::DropTable(c) => c.table_name.clone(),
            SchemaChange::RenameTable(c) => c.old_name.clone(),
            SchemaChange::AddColumn(c) => c.table_name.clone(),
            SchemaChange::DropColumn(c) => c.table_name.clone(),
            SchemaChange::AlterColumn(c) => c.table_name.clone(),
            SchemaChange::RenameColumn(c) => c.table_name.clone(),
            SchemaChange::ReorderColumns(c) => c.table_name.clone(),
            SchemaChange::CreateIndex(c) => c.table_name.clone(),
            SchemaChange::DropIndex(c) => c.table_name.clone(),
            SchemaChange::AddForeignKey(c) => c.table_name.clone(),
            SchemaChange::DropForeignKey(c) => c.table_name.clone(),
            SchemaChange::AddCheckConstraint(c) => c.table_name.clone(),
            SchemaChange::DropCheckConstraint(c) => c.table_name.clone(),
            SchemaChange::CreateMaterializedView(c) => c.view_name.clone(),
            SchemaChange::DropMaterializedView(c) => c.view_name.clone(),
            SchemaChange::ChangeKeyStructure(c) => c.table_name.clone(),
            SchemaChange::ChangeKey(c) => c.table_name.clone(),
            SchemaChange::ChangeReader(c) => c.table_name.clone(),
            SchemaChange::ChangeWriter(c) => c.table_name.clone(),
            SchemaChange::ChangePlacement(c) => c.table_name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn destructive_migration_is_blocked_and_the_refusal_is_logged() {
        // The safety gate is an audit point: a refusal must leave a log line saying why.
        let manager = MigrationManager::with_defaults(); // is_protected = false
        let migration = Migration {
            name: "drop_users".to_string(),
            generated_at: Utc::now(),
            database: MigrationDatabase::SqlDb,
            up_statements: vec![],
            down_statements: vec![],
            changes: vec![],
            safety_tier: SafetyTier::Destructive,
            applied: false,
            applied_at: None,
        };

        let result = manager.check_safety(&migration);
        assert!(
            matches!(result, Err(MigrationError::SafetyCheck { .. })),
            "a destructive migration must be refused"
        );
        assert!(
            logs_contain("migration blocked by safety check"),
            "the refusal must be logged for the audit trail"
        );
        assert!(
            logs_contain("destructive_changes"),
            "the refusal must record why it was blocked"
        );
    }
}
