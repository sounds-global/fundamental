//! Configuration for the migration manager.

use std::path::PathBuf;

/// Configuration for the migration manager.
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Root directory for migrations.
    pub migrations_dir: PathBuf,
    /// Environment name (e.g., "development", "staging", "production").
    pub environment: String,
    /// Whether this is a protected environment (requires --force for apply).
    pub is_protected: bool,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            migrations_dir: PathBuf::from("migrations"),
            environment: "development".to_string(),
            is_protected: false,
        }
    }
}
