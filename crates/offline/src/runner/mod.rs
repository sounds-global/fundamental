//! Migration runner for offline (SQLite) databases.
//!
//! `PRAGMA foreign_keys` is a no-op inside a transaction, so the runner hoists a
//! migration's FK pragmas outside the transaction it wraps the body in, and runs
//! `PRAGMA foreign_key_check` before committing.

use fundamental_db::migration::split_statements;
use tracing::field::Empty;
use tracing::{Span, debug, error, info, instrument, trace};

use crate::db::{OfflineDatabase, OfflineDbError, Value};

/// Tracking table for applied migrations.
const TRACKING_TABLE: &str = "_fundamental_migrations";

/// Applies offline migrations in order, exactly once each.
#[derive(Debug, Default)]
pub struct OfflineMigrator;

impl OfflineMigrator {
    /// Creates a new migrator.
    pub fn new() -> Self {
        Self
    }

    /// Returns the names of all applied migrations, in application order.
    #[instrument(level = "debug", skip(self, db), fields(applied = Empty))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn applied(&self, db: &OfflineDatabase) -> Result<Vec<String>, OfflineDbError> {
        self.ensure_tracking_table(db)?;

        let rows = db.query(
            &format!("SELECT name FROM {TRACKING_TABLE} ORDER BY applied_at, name;"),
            &[],
        )?;

        let names: Vec<String> = rows
            .into_iter()
            .filter_map(|row| row.get(0).and_then(|v| v.as_text().map(str::to_string)))
            .collect();
        Span::current().record("applied", names.len());
        Ok(names)
    }

    /// Applies the pending `(name, sql)` migrations in order, returning the names applied.
    #[instrument(
        skip(self, db, migrations),
        fields(
            candidates = migrations.len(),
            applied = Empty,
            skipped = Empty,
        )
    )]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn apply_all(
        &self,
        db: &OfflineDatabase,
        migrations: &[(&str, &str)],
    ) -> Result<Vec<String>, OfflineDbError> {
        let applied = self.applied(db)?;
        let mut newly_applied = Vec::new();
        let mut skipped = 0usize;

        for (name, sql) in migrations {
            if applied.iter().any(|a| a == name) {
                debug!(migration = %name, "migration already applied; skipping");
                skipped += 1;
                continue;
            }
            info!(migration = %name, "applying offline migration");
            self.apply_one(db, name, sql)?;
            newly_applied.push((*name).to_string());
        }

        let span = Span::current();
        span.record("applied", newly_applied.len());
        span.record("skipped", skipped);
        info!(
            applied = newly_applied.len(),
            skipped, "offline migrations up to date"
        );
        Ok(newly_applied)
    }

    /// Applies a single migration (unconditionally) and records it.
    #[instrument(
        skip(self, db, sql),
        fields(
            migration = name,
            statements = Empty,
            hoist_fk = Empty,
        )
    )]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn apply_one(&self, db: &OfflineDatabase, name: &str, sql: &str) -> Result<(), OfflineDbError> {
        let statements = split_statements(sql);
        let mut body = Vec::new();
        let mut has_fk_pragmas = false;

        for stmt in &statements {
            match classify(stmt) {
                StatementKind::ForeignKeysPragma => has_fk_pragmas = true,
                // The runner runs its own check.
                StatementKind::ForeignKeyCheck => has_fk_pragmas = true,
                StatementKind::Body => body.push(stmt.as_str()),
            }
        }

        let span = Span::current();
        span.record("statements", body.len());
        span.record("hoist_fk", has_fk_pragmas);

        // Hoisted outside the transaction: a no-op inside one.
        if has_fk_pragmas {
            debug!("hoisting foreign_keys pragma outside the migration transaction");
            db.execute_batch("PRAGMA foreign_keys=OFF;")?;
        }

        let result = self.apply_in_transaction(db, name, &body, has_fk_pragmas);

        if has_fk_pragmas {
            // Restore enforcement on success and failure alike.
            let restore = db.execute_batch("PRAGMA foreign_keys=ON;");
            result?;
            restore?;
            Ok(())
        } else {
            result
        }
    }

    #[instrument(
        skip(self, db, body),
        fields(migration = name, body_statements = body.len(), check_foreign_keys)
    )]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn apply_in_transaction(
        &self,
        db: &OfflineDatabase,
        name: &str,
        body: &[&str],
        check_foreign_keys: bool,
    ) -> Result<(), OfflineDbError> {
        let failed = |message: String| OfflineDbError::MigrationFailed {
            name: name.to_string(),
            message,
        };

        let tx = db.transaction().map_err(|e| failed(e.to_string()))?;

        for (i, stmt) in body.iter().enumerate() {
            trace!(
                statement = i + 1,
                total = body.len(),
                "applying migration statement"
            );
            tx.execute_batch(stmt).map_err(|e| failed(e.to_string()))?;
        }

        if check_foreign_keys {
            let violations = tx
                .query("PRAGMA foreign_key_check;", &[])
                .map_err(|e| failed(e.to_string()))?;
            if !violations.is_empty() {
                // Dropping `tx` rolls back.
                error!(
                    violations = violations.len(),
                    "migration foreign key check failed; rolling back"
                );
                return Err(failed(format!(
                    "foreign key check reported {} violation(s)",
                    violations.len()
                )));
            }
            debug!("migration foreign key check passed");
        }

        tx.execute(
            &format!(
                "INSERT INTO {TRACKING_TABLE} (name, applied_at) VALUES (?1, datetime('now'));"
            ),
            &[Value::from(name)],
        )
        .map_err(|e| failed(e.to_string()))?;

        tx.commit().map_err(|e| failed(e.to_string()))?;
        info!("offline migration applied and committed");
        Ok(())
    }

    #[instrument(level = "trace", skip(self, db))]
    fn ensure_tracking_table(&self, db: &OfflineDatabase) -> Result<(), OfflineDbError> {
        db.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {TRACKING_TABLE} (\n    \
                 name TEXT NOT NULL PRIMARY KEY,\n    \
                 applied_at TEXT NOT NULL\n\
             );"
        ))
    }
}

enum StatementKind {
    /// `PRAGMA foreign_keys=ON/OFF` — hoisted outside the transaction.
    ForeignKeysPragma,
    /// `PRAGMA foreign_key_check` — replaced by the runner's own check.
    ForeignKeyCheck,
    /// Everything else.
    Body,
}

fn classify(stmt: &str) -> StatementKind {
    let normalized = stmt
        .trim()
        .trim_end_matches(';')
        .to_uppercase()
        .replace(' ', "");
    if normalized.starts_with("PRAGMAFOREIGN_KEYS=") {
        StatementKind::ForeignKeysPragma
    } else if normalized == "PRAGMAFOREIGN_KEY_CHECK" {
        StatementKind::ForeignKeyCheck
    } else {
        StatementKind::Body
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::db::{OfflineConfig, OfflineDatabase};
    use tracing_test::traced_test;

    #[traced_test]
    #[tokio::test]
    async fn apply_logs_each_migration_and_logs_skips() {
        let db = OfflineDatabase::open(OfflineConfig::memory())
            .await
            .expect("open in-memory db");
        let migrator = OfflineMigrator::new();
        let migrations = [("0001_init", "CREATE TABLE a (id INTEGER PRIMARY KEY);")];

        let applied = migrator.apply_all(&db, &migrations).expect("first apply");
        assert_eq!(applied, vec!["0001_init".to_string()]);
        assert!(
            logs_contain("applying offline migration"),
            "each applied migration must be logged at info"
        );

        // Re-running applies nothing.
        let again = migrator.apply_all(&db, &migrations).expect("second apply");
        assert!(again.is_empty());
        assert!(
            logs_contain("migration already applied"),
            "an already-applied migration must log the skip"
        );
    }

    #[test]
    fn test_classify_fk_pragmas() {
        assert!(matches!(
            classify("PRAGMA foreign_keys=OFF;"),
            StatementKind::ForeignKeysPragma
        ));
        assert!(matches!(
            classify("pragma foreign_keys = ON"),
            StatementKind::ForeignKeysPragma
        ));
        assert!(matches!(
            classify("PRAGMA foreign_key_check;"),
            StatementKind::ForeignKeyCheck
        ));
        assert!(matches!(
            classify("CREATE TABLE t (x TEXT);"),
            StatementKind::Body
        ));
    }
}
