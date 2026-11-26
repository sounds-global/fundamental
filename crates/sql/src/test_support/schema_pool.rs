use std::time::Duration;

use sqlx::Row;
use tracing::debug;

use super::{PooledSchemaName, is_inlinable};
use crate::{SqlConfig, SqlError, SqlPool, SqlResult};

/// The registry table, in `public` of the app's own database, that makes a lease an atomic write.
const REGISTRY: &str = "public.fundamental_pooled_schemas";

/// How long a row is held while its schema is being dropped; bounds a retirement killed half-way.
const RETIREMENT_LEASE: Duration = Duration::from_secs(300);

/// The registry of reusable per-test schemas, leased by a single `UPDATE … RETURNING`.
///
/// `nextest` runs every test in its own process, so pooling moves the DDL cost from per test
/// to per migration set. A lease expires, so a killed test cannot leak its schema.
pub struct SchemaPool {
    /// Addresses `public`, the registry, never a leased schema.
    pool: SqlPool,
    /// The migration set these schemas were built from, as a hex digest; another digest is another pool.
    fingerprint: String,
}

impl SchemaPool {
    /// Connects to the database `database_url` addresses, for the pool of `fingerprint`.
    ///
    /// # Errors
    /// Returns the underlying [`SqlError`](crate::SqlError) if the database cannot be reached.
    pub async fn connect(database_url: &str, fingerprint: &str) -> SqlResult<Self> {
        let pool = SqlPool::connect(&SqlConfig::development(database_url)).await?;
        Ok(Self {
            pool,
            fingerprint: fingerprint.to_owned(),
        })
    }

    /// Makes sure the registry table exists, whoever creates it.
    ///
    /// # Errors
    /// Returns the underlying [`SqlError`](crate::SqlError) only if the registry is still
    /// absent after the attempt.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub async fn ensure_registry(&self) -> SqlResult<()> {
        if self.registry_exists().await? {
            return Ok(());
        }
        let created = self
            .pool
            .execute_ddl(&format!(
                "CREATE TABLE IF NOT EXISTS {REGISTRY} (\
                   schema_name  TEXT PRIMARY KEY, \
                   fingerprint  TEXT NOT NULL, \
                   leased_until TIMESTAMPTZ NOT NULL)"
            ))
            .await;
        match created {
            Ok(_) => Ok(()),
            // `already exists` is the postcondition; re-reading the catalog can lag the commit that beat us.
            Err(error) if is_already_exists(&error) => {
                debug!(%error, "another process created the schema registry first");
                Ok(())
            }
            Err(error) => {
                if self.registry_exists().await.unwrap_or(false) {
                    debug!(%error, "the registry could not be created and is there anyway");
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }

    /// Whether the registry table is in the catalog; `::text` because no driver decodes `regclass`.
    async fn registry_exists(&self) -> SqlResult<bool> {
        let found = self
            .pool
            .fetch_optional(&format!(
                "SELECT to_regclass('{REGISTRY}')::text AS relation"
            ))
            .await?;
        match found {
            Some(row) => Ok(row.try_get::<Option<String>, _>(0)?.is_some()),
            None => Ok(false),
        }
    }

    /// Takes the lease on one free schema, or `None` when every schema in the pool is held.
    ///
    /// The `UPDATE`'s own `WHERE` re-checks the candidate, so a claimant that lost the race leases nothing.
    ///
    /// # Errors
    /// Returns the underlying [`SqlError`](crate::SqlError) if the statement fails.
    pub async fn claim(&self, lease: Duration) -> SqlResult<Option<String>> {
        let seconds = lease.as_secs();
        let fingerprint = &self.fingerprint;
        let claimed = self
            .pool
            .fetch_optional(&format!(
                "UPDATE {REGISTRY} \
                    SET leased_until = now() + interval '{seconds} seconds' \
                  WHERE schema_name = ( \
                        SELECT schema_name FROM {REGISTRY} \
                         WHERE fingerprint = '{fingerprint}' AND leased_until < now() \
                         ORDER BY leased_until LIMIT 1) \
                    AND leased_until < now() \
              RETURNING schema_name"
            ))
            .await?;
        match claimed {
            Some(row) => Ok(Some(row.try_get::<String, _>(0)?)),
            None => Ok(None),
        }
    }

    /// Takes the slot `schema` names, if no other builder has it, and leases it to the caller;
    /// `true` when the caller now holds it. A row always precedes its schema.
    ///
    /// # Errors
    /// Returns the underlying [`SqlError`](crate::SqlError) if the insert fails for any
    /// reason other than the slot already being taken.
    pub async fn reserve(&self, schema: &str, lease: Duration) -> SqlResult<bool> {
        if !is_inlinable(schema) {
            eprintln!("schema pool: refusing to reserve '{schema}'; it is not a plain name");
            return Ok(false);
        }
        let seconds = lease.as_secs();
        let fingerprint = &self.fingerprint;
        let taken = self
            .pool
            .fetch_optional(&format!(
                "INSERT INTO {REGISTRY} (schema_name, fingerprint, leased_until) \
                 VALUES ('{schema}', '{fingerprint}', now() + interval '{seconds} seconds') \
                 ON CONFLICT (schema_name) DO NOTHING \
                 RETURNING schema_name"
            ))
            .await?;
        Ok(taken.is_some())
    }

    /// How many schemas this pool holds, leased or not.
    ///
    /// # Errors
    /// Returns the underlying [`SqlError`](crate::SqlError) if the count fails.
    pub async fn size(&self) -> SqlResult<i64> {
        let fingerprint = &self.fingerprint;
        let counted = self
            .pool
            .fetch_optional(&format!(
                "SELECT count(*) FROM {REGISTRY} WHERE fingerprint = '{fingerprint}'"
            ))
            .await?;
        match counted {
            Some(row) => Ok(row.try_get::<i64, _>(0)?),
            None => Ok(0),
        }
    }

    /// Removes unleased schemas built from another migration set or not named as a
    /// [`PooledSchemaName`], and their rows, leasing each row first; returns how many.
    ///
    /// A row is given up even when its drop fails, so a half-dropped schema is never leased.
    ///
    /// # Errors
    /// Returns the underlying [`SqlError`](crate::SqlError) if the listing fails. A single
    /// schema that resists removal is reported on stderr and skipped.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub async fn retire_stale(&self) -> SqlResult<usize> {
        // Judged in Rust, so the name grammar has one definition.
        let free = self
            .pool
            .fetch_all(&format!(
                "SELECT schema_name::text, fingerprint::text FROM {REGISTRY} \
                  WHERE leased_until < now() \
                  ORDER BY schema_name LIMIT 64"
            ))
            .await?;

        let mut removed = 0;
        for row in &free {
            let name: String = row.try_get(0)?;
            let fingerprint: String = row.try_get(1)?;
            if fingerprint == self.fingerprint && PooledSchemaName::is_pooled(&name) {
                continue;
            }
            if !is_inlinable(&name) {
                eprintln!("schema pool: refusing to inline '{name}'; it was left alone");
                continue;
            }
            if !self.lease_for_retirement(&name).await? {
                debug!(name, "pool: a stale row was claimed by somebody else first");
                continue;
            }

            let dropped = self
                .pool
                .execute_ddl(&format!("DROP SCHEMA IF EXISTS {name} CASCADE"))
                .await;
            if let Err(error) = &dropped {
                eprintln!(
                    "schema pool: DROP SCHEMA {name} failed and its row is given up anyway, so \
                     nothing leases a half-dropped namespace; the sweep that owns the name will \
                     take it: {error}"
                );
            }
            if let Err(error) = self
                .pool
                .execute(&format!(
                    "DELETE FROM {REGISTRY} WHERE schema_name = '{name}'"
                ))
                .await
            {
                eprintln!("schema pool: {name} was retired and its row remains: {error}");
            } else if dropped.is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Takes the lease on one named row, so retiring it cannot race a claimant.
    async fn lease_for_retirement(&self, schema: &str) -> SqlResult<bool> {
        let seconds = RETIREMENT_LEASE.as_secs();
        let taken = self
            .pool
            .fetch_optional(&format!(
                "UPDATE {REGISTRY} \
                    SET leased_until = now() + interval '{seconds} seconds' \
                  WHERE schema_name = '{schema}' AND leased_until < now() \
              RETURNING schema_name"
            ))
            .await?;
        Ok(taken.is_some())
    }

    /// Removes pooled schemas no registry row names, which only an abandoned build leaves; returns how many.
    ///
    /// # Errors
    /// Returns the underlying [`SqlError`](crate::SqlError) if the listing fails. A single
    /// schema that resists removal is reported on stderr and skipped.
    pub async fn retire_orphans(&self) -> SqlResult<usize> {
        // Filtered in Rust, so the grammar has one definition, in `PooledSchemaName`.
        let named = self
            .pool
            .fetch_all(&format!(
                "SELECT catalog.schema_name::text \
                   FROM information_schema.schemata catalog \
                   LEFT JOIN {REGISTRY} registry \
                     ON registry.schema_name = catalog.schema_name::text \
                  WHERE registry.schema_name IS NULL \
                  ORDER BY catalog.schema_name LIMIT 256"
            ))
            .await?;

        let mut removed = 0;
        for row in &named {
            let name: String = row.try_get(0)?;
            if !PooledSchemaName::is_pooled(&name) || !is_inlinable(&name) {
                continue;
            }
            match self
                .pool
                .execute_ddl(&format!("DROP SCHEMA IF EXISTS {name} CASCADE"))
                .await
            {
                Ok(_) => {
                    debug!(name, "pool: retired a pooled schema no registry row named");
                    removed += 1;
                }
                Err(error) => eprintln!("schema pool: DROP SCHEMA {name} failed: {error}"),
            }
        }
        Ok(removed)
    }

    /// The registry's connection.
    #[must_use]
    pub fn pool(&self) -> &SqlPool {
        &self.pool
    }

    /// The migration digest this pool leases for.
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

/// Whether the engine refused a `CREATE` as already there (`42P07` or `42710`), read from the SQLSTATE.
fn is_already_exists(error: &SqlError) -> bool {
    let SqlError::Sqlx(sqlx::Error::Database(database)) = error else {
        return false;
    };
    matches!(database.code().as_deref(), Some("42P07" | "42710"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opening_an_unreachable_database_is_an_error_and_not_a_panic() {
        // Unroutable: the suite turns this refusal into "skip", so it must not hang or unwrap.
        let refused = SchemaPool::connect(
            "postgresql://yugabyte@127.0.0.1:1/base?sslmode=disable",
            "ab",
        )
        .await
        .is_err();
        assert!(
            refused,
            "an unreachable database must be reported as an error"
        );
    }

    #[test]
    fn a_name_that_could_end_a_literal_is_never_registered() {
        // `register` inlines the name into SQL, so this guard is the whole defence.
        assert!(!is_inlinable("t'; DROP SCHEMA prod; --"));
    }
}
