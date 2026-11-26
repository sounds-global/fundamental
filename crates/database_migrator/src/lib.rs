// The ephemeral fixtures' nested futures exceed rustc's default layout depth on the test target.
#![recursion_limit = "256"]

//! Migration formatting and execution for the Fundamental framework.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

// Development only: their pools use the TLS-less `development` configs, which exist only under
// `debug_assertions`, so compiling them unconditionally breaks a release build.
#[cfg(feature = "sql")]
mod base_files;
#[cfg(all(debug_assertions, feature = "test-support"))]
mod ephemeral;
mod formatters;
mod manager;
// A runner is its engines' methods; with no engine enabled there is nothing for one to run.
mod migrate_command;
#[cfg(any(feature = "sql", feature = "cql", feature = "offline"))]
mod migrate_runner;
mod migration_check;
mod migration_path;
#[cfg(all(debug_assertions, feature = "test-support", feature = "sql"))]
mod pooled;
#[cfg(any(feature = "sql", feature = "cql"))]
mod resolve;
mod run_main;
#[cfg(feature = "cql")]
mod scratch_keyspace;
#[cfg(feature = "sql")]
mod scratch_schema;
#[cfg(feature = "sql")]
mod verify;

#[cfg(all(debug_assertions, feature = "test-support"))]
pub use ephemeral::EphemeralError;
#[cfg(all(debug_assertions, feature = "test-support", feature = "cql"))]
pub use ephemeral::EphemeralKeyspace;
#[cfg(all(debug_assertions, feature = "test-support", feature = "sql"))]
pub use ephemeral::EphemeralSchema;
pub use formatters::*;
pub use manager::MigrationManager;
pub use migrate_command::MigrateCommand;
#[cfg(any(feature = "sql", feature = "cql", feature = "offline"))]
pub use migrate_runner::MigrateRunner;
pub use migration_check::{DRIFT_BLIND_SPOTS, MigrationCheck};
#[cfg(all(debug_assertions, feature = "test-support", feature = "sql"))]
pub use pooled::PooledSchema;
pub use run_main::run_main;
#[cfg(feature = "sql")]
pub use verify::Verification;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fundamental_db::migration::{
        AddColumnChange, DropTableChange, Migration, MigrationConfig, MigrationDatabase,
        SafetyTier, SchemaChange, TableColumn,
    };
    use tempfile::TempDir;

    fn manager_in(environment: &str, is_protected: bool) -> (MigrationManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = MigrationConfig {
            migrations_dir: temp_dir.path().to_path_buf(),
            environment: environment.to_string(),
            is_protected,
        };
        (MigrationManager::new(config), temp_dir)
    }

    /// Builds a migration value by hand, carrying `changes`, so the safety classification
    /// can be tested on it.
    fn migration_of(name: &str, changes: Vec<SchemaChange>) -> Migration {
        Migration {
            name: name.to_string(),
            generated_at: Utc::now(),
            database: MigrationDatabase::SqlDb,
            up_statements: vec![],
            down_statements: vec![],
            safety_tier: SafetyTier::from_changes(&changes),
            changes,
            applied: false,
            applied_at: None,
        }
    }

    #[test]
    fn a_dropped_table_is_destructive_and_refused() {
        let (manager, _temp) = manager_in("test", false);
        let migration = migration_of(
            "drop_users",
            vec![SchemaChange::DropTable(DropTableChange {
                table_name: "users".to_string(),
                cascade: false,
            })],
        );

        assert!(migration.is_destructive());
        assert!(
            manager.check_safety(&migration).is_err(),
            "a destructive change is refused in every environment"
        );
    }

    #[test]
    fn a_review_level_change_is_refused_in_a_protected_environment() {
        let (manager, _temp) = manager_in("production", true);
        let migration = migration_of(
            "add_status",
            vec![SchemaChange::AddColumn(AddColumnChange {
                table_name: "users".to_string(),
                column: TableColumn {
                    name: "status".to_string(),
                    data_type: "TEXT".to_string(),
                    // Non-nullable with no default: it rejects every existing row.
                    nullable: false,
                    default: None,
                    primary_key: false,
                    unique: false,
                },
            })],
        );

        assert!(manager.check_safety(&migration).is_err());
    }
}
