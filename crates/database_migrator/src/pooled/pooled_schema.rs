//! [`PooledSchema`] — a migrated schema a test **leases** instead of building.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use fundamental_db::SchemaName;
use fundamental_db::migration::{MigrationConfig, MigrationGuard, content_hash};
use fundamental_sql::test_support::{
    LeasedSchema, PooledSchemaName, SchemaPool, schema_exists, wipe_schema,
};
use fundamental_sql::{SqlConfig, SqlPool};
use tracing::field::Empty;
use tracing::{Span, debug, instrument, warn};

use crate::{EphemeralError, MigrateRunner};

/// The longest schema name this engine accepts.
const MAX_SCHEMA_NAME: usize = 63;

/// How long a lease is held before the pool considers it abandoned: longer than any test a
/// harness lets run, so an expired lease means the holder is gone.
const LEASE: Duration = Duration::from_secs(900);

/// The most schemas one migration set may hold in one database, and its slot count: schema `n`
/// is `<prefix>pool_<digest>_<n>`, so lowering it strands the schemas above the bound until
/// [`SchemaPool::retire_orphans`] removes them.
const MAX_POOLED: u32 = 16;

/// How many times acquiring asks for a lease before giving up.
const CLAIM_ATTEMPTS: u32 = 60;

/// How long to wait between claims when the whole pool is leased.
const CLAIM_BACKOFF: Duration = Duration::from_millis(250);

/// How many times a slot's schema is built, as a whole, before the failure reaches the caller.
const BUILD_ATTEMPTS: u32 = 3;

/// How long to wait after a failed build before a fresh one, doubled each time.
const BUILD_BACKOFF: Duration = Duration::from_millis(750);

/// How many times emptying a leased slot is attempted: the wipe meets the same cluster-wide
/// catalog-latch `40001` the migrator does, so it gets the migrator's budget.
const WIPE_ATTEMPTS: u32 = 24;

/// The databases whose stale schemas this process has already retired.
fn retired() -> &'static Mutex<HashSet<String>> {
    static RETIRED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    RETIRED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// A freshly-emptied, already-migrated schema, leased for the life of one test.
///
/// Provisioning happens once per migration set; later tests lease a schema, empty it and give it
/// back. A test that changes its schema's shape keeps [`EphemeralSchema`](crate::EphemeralSchema):
/// emptying rows does not undo DDL.
pub struct PooledSchema {
    /// First field: fields drop in order, so the pool closes before the lease is handed back.
    pool: SqlPool,
    url: String,
    schema: SchemaName,
    /// The lease, and the promise to hand it back. Its `Drop` is this type's teardown.
    leased: LeasedSchema,
}

impl PooledSchema {
    /// Leases a schema built from `migrations_dir` in the database `database_url` addresses,
    /// building one first if the pool has none free.
    ///
    /// `prefix` names the minted schemas and includes its trailing separator, exactly as
    /// [`EphemeralSchema::provision`](crate::EphemeralSchema::provision) takes it.
    ///
    /// # Errors
    /// Returns [`EphemeralError::Unreachable`] if the database cannot be contacted — the
    /// signal a suite turns into "skip" — and [`Create`](EphemeralError::Create) or
    /// [`Migrate`](EphemeralError::Migrate) if it answered and the pool could neither lease
    /// nor build a schema.
    #[instrument(skip_all, fields(prefix = prefix, name = Empty, built = Empty), err(Debug))]
    pub async fn acquire(
        database_url: &str,
        prefix: &str,
        migrations_dir: impl Into<PathBuf>,
        guard: MigrationGuard,
    ) -> Result<Self, EphemeralError> {
        let migrations_dir = migrations_dir.into();
        let fingerprint = fingerprint_of(&migrations_dir);
        // Two steps: an unreachable cluster is a skip, an unbuildable registry is a defect.
        let registry = SchemaPool::connect(database_url, &fingerprint)
            .await
            .map_err(|error| EphemeralError::Unreachable {
                detail: error.to_string(),
            })?;
        registry
            .ensure_registry()
            .await
            .map_err(|error| EphemeralError::Create {
                name: prefix.to_owned(),
                detail: format!("the pool's registry could not be created: {error}"),
            })?;

        retire_once(database_url, &fingerprint, &registry).await;

        let mut refused: Option<String> = None;
        for attempt in 1..=CLAIM_ATTEMPTS {
            match registry.claim(LEASE).await {
                Ok(Some(name)) => {
                    // Held before anything fallible, so every `?` below frees the lease.
                    let leased = LeasedSchema::held(database_url, &name);
                    return Self::take(
                        database_url,
                        leased,
                        &registry,
                        migrations_dir.clone(),
                        guard.clone(),
                    )
                    .await;
                }
                // A conflict means somebody else took it: ask again.
                Err(error) => {
                    if refused.is_none() {
                        warn!(attempt, %error, "the pool's registry refused a claim; asking again");
                    } else {
                        debug!(attempt, %error, "the pool's registry refused a claim again");
                    }
                    refused = Some(error.to_string());
                }
                Ok(None) => {
                    if let Some(built) = Self::build(
                        database_url,
                        prefix,
                        migrations_dir.clone(),
                        guard.clone(),
                        &registry,
                    )
                    .await?
                    {
                        Span::current().record("built", true);
                        return Ok(built);
                    }
                }
            }
            tokio::time::sleep(CLAIM_BACKOFF).await;
        }

        Err(EphemeralError::Create {
            name: prefix.to_owned(),
            detail: exhaustion_detail(refused.as_deref()),
        })
    }

    /// Reconciles the schema `leased` names to what a freshly-migrated one would be.
    ///
    /// One path whichever way in: brought up to every migration on disk, emptied and re-seeded.
    /// Completeness is asked of the ledger, not of the schema's existence: a builder killed
    /// mid-migration leaves the namespace present and its tables incomplete.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "readability baseline"
    )]
    async fn take(
        database_url: &str,
        leased: LeasedSchema,
        registry: &SchemaPool,
        migrations_dir: PathBuf,
        guard: MigrationGuard,
    ) -> Result<Self, EphemeralError> {
        let name = leased.name().to_owned();
        Span::current().record("name", &name);
        let schema = SchemaName::new(name.clone()).map_err(|error| EphemeralError::Create {
            name: name.clone(),
            detail: format!("the pool holds a name that is not a schema identifier: {error}"),
        })?;

        let present = schema_exists(registry.pool(), &name)
            .await
            .map_err(|error| EphemeralError::Create {
                name: name.clone(),
                detail: format!("the catalog could not be asked for the schema: {error}"),
            })?;

        let pool =
            SqlPool::connect(&SqlConfig::development(database_url).default_schema(schema.clone()))
                .await
                .map_err(|error| EphemeralError::Unreachable {
                    detail: error.to_string(),
                })?;

        let runner = MigrateRunner::new(
            MigrationConfig {
                migrations_dir,
                environment: "development".to_owned(),
                is_protected: false,
            },
            guard,
        );

        // One indexed read and no DDL on the hot path: a complete slot is the common case.
        let complete = present && Self::is_fully_migrated(&pool, &name, &runner).await;
        if !complete {
            build_into(&pool, &schema, &runner).await?;
            if present {
                debug!(name, "an incomplete pooled slot was migrated up and leased");
            } else {
                debug!(name, "schema built into its pooled slot and leased");
            }
        }

        // Wiped on acquire, not on release: a killed holder ran no cleanup.
        let registry_pool = registry.pool();
        registry_pool
            .with_retry_budget(WIPE_ATTEMPTS, || async {
                wipe_schema(registry_pool, &name).await
            })
            .await
            .map_err(|error| EphemeralError::Create {
                name: name.clone(),
                detail: format!("the leased schema could not be emptied: {error}"),
            })?;

        // A wipe removes the migrations' baseline rows too; replaying the data statements restores
        // them.
        runner
            .replay_sql_data(&pool)
            .await
            .map_err(|error| EphemeralError::Migrate {
                name: name.clone(),
                detail: format!("the leased schema's baseline could not be replayed: {error}"),
            })?;
        if complete {
            debug!(name, "schema leased from the pool");
        }

        Ok(Self {
            pool,
            url: database_url.to_owned(),
            schema,
            leased,
        })
    }

    /// Whether every migration on disk has a ledger row in this slot; `false` on an unreadable
    /// ledger.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    async fn is_fully_migrated(pool: &SqlPool, name: &str, runner: &MigrateRunner) -> bool {
        match runner.unapplied_sql(pool).await {
            Ok(unapplied) if unapplied.is_empty() => true,
            Ok(unapplied) => {
                warn!(
                    name,
                    unapplied = unapplied.len(),
                    first = unapplied.first().map(String::as_str).unwrap_or_default(),
                    "pool: a slot was present but had not applied every migration; it is being \
                     migrated up before it is leased"
                );
                false
            }
            Err(error) => {
                warn!(
                    name,
                    %error,
                    "pool: a slot's migration ledger could not be read; it is being rebuilt \
                     rather than trusted"
                );
                false
            }
        }
    }

    /// Reserves the first free slot of this pool and returns the schema built into it, or
    /// `None` when every slot is taken.
    ///
    /// A pooled name is a function of `(prefix, fingerprint, slot)`, so racing builders propose one
    /// name and the registry's primary key decides.
    async fn build(
        database_url: &str,
        prefix: &str,
        migrations_dir: PathBuf,
        guard: MigrationGuard,
        registry: &SchemaPool,
    ) -> Result<Option<Self>, EphemeralError> {
        for slot in 0..MAX_POOLED {
            let name =
                PooledSchemaName::mint(prefix, registry.fingerprint(), slot, MAX_SCHEMA_NAME)?;

            let reserved = registry
                .reserve(name.as_str(), LEASE)
                .await
                .map_err(|error| EphemeralError::Create {
                    name: name.as_str().to_owned(),
                    detail: format!("the pool's slot could not be reserved: {error}"),
                })?;
            if !reserved {
                continue;
            }

            // Held before anything fallible: the reservation is written, so a failure must free it.
            let leased = LeasedSchema::held(database_url, name.as_str());
            return Self::take(database_url, leased, registry, migrations_dir, guard)
                .await
                .map(Some);
        }
        Ok(None)
    }

    /// The pool for this schema. Clone it to hand to code under test.
    #[must_use]
    pub fn pool(&self) -> &SqlPool {
        &self.pool
    }

    /// The leased schema's name, for diagnostics and for DDL that has to qualify it.
    #[must_use]
    pub fn name(&self) -> &str {
        self.leased.name()
    }

    /// The schema this fixture holds, for a caller that builds its own pool against it.
    #[must_use]
    pub fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// The connection URL of the database this schema lives in.
    ///
    /// A caller building its own pool from it must also apply [`schema`](Self::schema), or it
    /// addresses `public`.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// Creates the schema `schema` names and applies the migration set into it.
///
/// Not [`EphemeralSchema::provision`](crate::EphemeralSchema): its sweep removes every schema
/// carrying the ephemeral grammar, the pool's own included.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn build_into(
    pool: &SqlPool,
    schema: &SchemaName,
    runner: &MigrateRunner,
) -> Result<(), EphemeralError> {
    let mut backoff = BUILD_BACKOFF;
    for attempt in 1..=BUILD_ATTEMPTS {
        if attempt > 1 {
            // The half-built schema is worth nothing, and would trip the next attempt.
            if let Err(error) = pool
                .execute_ddl(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
                .await
            {
                warn!(%schema, %error, "pool: a half-built slot could not be dropped before the retry");
            }
        }

        match create_and_migrate(pool, schema, runner).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt == BUILD_ATTEMPTS || error.is_unreachable() => {
                return Err(error);
            }
            Err(error) => {
                warn!(
                    attempt,
                    of = BUILD_ATTEMPTS,
                    backoff_ms = backoff.as_millis(),
                    %schema,
                    %error,
                    "building a pooled schema failed; the half-built one is dropped and the \
                     slot is tried again"
                );
                tokio::time::sleep(backoff).await;
                backoff *= 2;
            }
        }
    }

    // Reached only with a `BUILD_ATTEMPTS` of zero.
    Err(EphemeralError::Create {
        name: schema.to_string(),
        detail: "no build attempt was made".to_owned(),
    })
}

/// One create/verify/migrate cycle of [`build_into`].
async fn create_and_migrate(
    pool: &SqlPool,
    schema: &SchemaName,
    runner: &MigrateRunner,
) -> Result<(), EphemeralError> {
    // `execute_ddl`: DDL contends on a catalog latch, and the statement is idempotent.
    pool.execute_ddl(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
        .await
        .map_err(|error| EphemeralError::Create {
            name: schema.to_string(),
            detail: error.to_string(),
        })?;

    // Fail closed: `SET search_path` to a missing schema does not error, so ask the catalog.
    let present = schema_exists(pool, schema.as_str())
        .await
        .map_err(|error| EphemeralError::Create {
            name: schema.to_string(),
            detail: format!("could not confirm the schema exists: {error}"),
        })?;
    if !present {
        return Err(EphemeralError::Create {
            name: schema.to_string(),
            detail: "the CREATE reported success and the catalog does not have it".to_owned(),
        });
    }

    runner
        .apply_sql(pool)
        .await
        .map_err(|error| EphemeralError::Migrate {
            name: schema.to_string(),
            detail: error.to_string(),
        })?;
    Ok(())
}

/// Retires what no longer belongs to this pool, once per process per database.
///
/// Stale rows of another migration set, and schemas no row names, which a failed build abandoned.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn retire_once(database_url: &str, fingerprint: &str, registry: &SchemaPool) {
    let key = format!("{database_url}#{fingerprint}");
    {
        let Ok(mut retired) = retired().lock() else {
            warn!("pool: the retire registry is poisoned; stale schemas were not retired");
            return;
        };
        if !retired.insert(key) {
            return;
        }
    }
    match registry.retire_stale().await {
        Ok(0) => {}
        Ok(retired) => debug!(retired, "pool: retired schemas from an older migration set"),
        Err(error) => warn!(%error, "pool: could not retire stale schemas; leasing continues"),
    }
    match registry.retire_orphans().await {
        Ok(0) => {}
        Ok(retired) => debug!(
            retired,
            "pool: retired pooled schemas no registry row named"
        ),
        Err(error) => warn!(%error, "pool: could not retire orphaned schemas; leasing continues"),
    }
}

/// The digest that decides whether a pooled schema is the schema these migrations describe.
///
/// A BLAKE3 over every `.sql` file's statements; an unreadable directory yields a digest that
/// matches nothing, so the pool builds rather than leases.
fn fingerprint_of(migrations_dir: &Path) -> String {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_sql(migrations_dir, &mut files);
    files.sort();

    let mut corpus = String::new();
    for file in &files {
        if let Ok(text) = std::fs::read_to_string(file) {
            corpus.push_str(
                &file
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            );
            corpus.push_str(";\n");
            corpus.push_str(&text);
            corpus.push_str(";\n");
        }
    }
    content_hash(&corpus)
}

/// Every `.sql` file under `directory`, recursively.
fn collect_sql(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sql(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "sql") {
            found.push(path);
        }
    }
}

/// Why no schema could be leased, naming the store's own refusal when there was one.
///
/// A registry refusing every claim and a pool whose slots are all held have different fixes.
fn exhaustion_detail(refused: Option<&str>) -> String {
    match refused {
        Some(error) => {
            format!("the pool's registry refused the last of {CLAIM_ATTEMPTS} claims: {error}")
        }
        None => format!(
            "every one of the {MAX_POOLED} pooled schemas stayed leased for {CLAIM_ATTEMPTS} \
             attempts; the harness runs more store-bound tests at once than the pool holds"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_that_does_not_exist_still_has_a_digest() {
        // It must not panic: the migration run below reports the real problem, and a fixture
        // that died computing a fingerprint would replace that message with a worse one.
        let digest = fingerprint_of(Path::new("/nonexistent/migrations"));
        assert!(!digest.is_empty());
    }

    #[test]
    fn changing_a_statement_changes_the_pool() {
        let one = content_hash("001.sql;\nCREATE TABLE t (id UUID PRIMARY KEY);\n;\n");
        let two = content_hash("001.sql;\nCREATE TABLE t (id UUID, name TEXT);\n;\n");
        assert_ne!(
            one, two,
            "a schema built from different statements must never be leased as this one"
        );
    }

    #[tokio::test]
    async fn an_unreachable_database_is_a_skip_and_not_a_failure() {
        // Unroutable: a suite turns `Unreachable` into a skip and everything else into red.
        let refused = PooledSchema::acquire(
            "postgresql://yugabyte@127.0.0.1:1/base?sslmode=disable",
            "layer5_test_",
            "migrations",
            MigrationGuard::unenforced(),
        )
        .await;
        assert!(matches!(refused, Err(EphemeralError::Unreachable { .. })));
    }

    #[test]
    fn a_refused_claim_is_named_not_reported_as_a_full_pool() {
        let refused = super::exhaustion_detail(Some("Write rejected: insufficient disk space"));
        assert!(refused.contains("insufficient disk space"), "{refused}");
        assert!(!refused.contains("stayed leased"), "{refused}");
        assert!(super::exhaustion_detail(None).contains("stayed leased"));
    }
}
