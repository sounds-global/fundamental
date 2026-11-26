//! [`Verification`] — the migrations replayed into empty schemas and read back, from scratch and
//! as the real upgrade from a base revision, as [`MigrateRunner::verify_sql`] does it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use fundamental_db::DatabaseError;
use fundamental_db::migration::generate::{SchemaDifference, SchemaItem, Snapshot};
use fundamental_db::migration::{
    MigrationDatabase, MigrationError, MigrationResult, Revision, SchemaIntrospector as _,
};
use fundamental_sql::SqlPool;
use fundamental_sql::migration::SqlIntrospector;
use fundamental_sql::sqlx::Row as _;
use tracing::field::Empty;
use tracing::{Span, info, instrument};

use crate::MigrateRunner;
use crate::base_files::{Directory, Upgrade, upgrade};
use crate::scratch_schema::ScratchSchema;

/// What [`MigrateRunner::verify_sql`] replayed, once the catalog it made agreed with the
/// committed snapshots, the models and itself.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Verification {
    /// The base revision's commit.
    pub base: String,
    /// The files replayed from scratch.
    pub from_scratch: usize,
    /// The files the upgrade applied as the base holds them.
    pub at_base: usize,
    /// The files the upgrade applied after them, which this branch adds.
    pub branch: usize,
}

impl Verification {
    /// One line a person reads: what was replayed, and that it agreed.
    #[must_use]
    pub fn summary(&self) -> String {
        let short = self.base.get(..12).unwrap_or(&self.base);
        format!(
            "from scratch {} files; upgrade {} files as at {short}, then {} of this branch; the \
             catalog is what the committed snapshots and the models say",
            self.from_scratch, self.at_base, self.branch
        )
    }
}

impl MigrateRunner {
    /// Replays every SQL migration into empty schemas beside `pool`'s and reads each catalog
    /// back: the real upgrade, and a replay from scratch.
    ///
    /// The upgrade applies each file as the commit `base` holds it, then the files this branch
    /// adds; the replay from scratch applies every file. Both catalogs are compared with the
    /// committed snapshots, the models and each other.
    ///
    /// # Errors
    /// [`MigrationError::AppliedFileChanged`], [`MigrationError::BaseFileGone`] or
    /// [`MigrationError::FileBeforeBase`], naming every file of that kind;
    /// [`MigrationError::BaseUnreadable`] when `base` names no commit of the working
    /// directory's repository; [`MigrationError::Unverified`] with every difference found; or
    /// what applying a file or reading a catalog failed with.
    #[instrument(skip(self, pool), fields(commit = Empty, differences = Empty), err(Debug))]
    pub async fn verify_sql(&self, pool: &SqlPool, base: &str) -> MigrationResult<Verification> {
        let here = std::env::current_dir()?;
        let revision = Revision::resolve(base, &here)?;
        Span::current().record("commit", revision.commit());
        let directories = self.sql_directories(&revision)?;
        let plan = upgrade(
            revision.commit(),
            &directories
                .iter()
                .map(|(directory, _)| directory.clone())
                .collect::<Vec<_>>(),
        )?;
        let committed = self.committed_snapshots()?;
        let models = Snapshot::of(
            &self
                .expected_schema(MigrationDatabase::SqlDb)
                .tables
                .into_values()
                .collect::<Vec<_>>(),
            None,
        );

        // Boxed: the replays nest the runner's whole apply path, and an unboxed future that deep
        // overflows the compiler's layout depth in the binary that awaits it.
        let replays = Box::pin(self.replays(pool, &directories, &plan)).await?;
        let report = replays.compare(&committed, &models);

        let verification = Verification {
            base: revision.commit().to_owned(),
            from_scratch: plan.at_base.len() + plan.new.len(),
            at_base: plan.at_base.len(),
            branch: plan.new.len(),
        };
        Span::current().record("differences", report.differences.len());
        if report.differences.is_empty() {
            info!(
                from_scratch = verification.from_scratch,
                at_base = verification.at_base,
                branch = verification.branch,
                "the migrations make what the snapshots and the models say"
            );
            return Ok(verification);
        }
        Err(MigrationError::Unverified {
            message: format!(
                "{} difference(s) replaying the migrations against {base} ({}):\n{}",
                report.differences.len(),
                revision.commit(),
                report.text.trim_end()
            ),
            differences: report.differences,
        })
    }

    /// The replay from scratch and the upgrade, each into a schema of its own made beside
    /// `pool`'s and removed once its catalog is read.
    async fn replays(
        &self,
        pool: &SqlPool,
        directories: &[(Directory, bool)],
        plan: &Upgrade,
    ) -> MigrationResult<Replays> {
        let scratch = ScratchSchema::create(pool, "scratch").await?;
        let replayed = self.replay(scratch.pool(), directories).await;
        scratch.remove(pool).await;
        let (scratch, scratch_unusable, by_hand) = replayed?;

        let upgraded = ScratchSchema::create(pool, "upgrade").await?;
        let applied = self.upgrade(upgraded.pool(), plan).await;
        upgraded.remove(pool).await;
        let (upgrade, upgrade_unusable) = applied?;
        Ok(Replays {
            scratch,
            scratch_unusable,
            upgrade,
            upgrade_unusable,
            by_hand,
        })
    }

    /// Each SQL directory in the order it applies, with whether a committed snapshot says what
    /// its files make: every embedded set, then the app's own.
    fn sql_directories(&self, revision: &Revision) -> MigrationResult<Vec<(Directory, bool)>> {
        let mut directories: Vec<(Directory, bool)> = Vec::new();
        for (set, migration) in self.embedded_for(MigrationDatabase::SqlDb)? {
            let owner = format!("{}/", set.owner());
            if directories
                .last()
                .is_none_or(|(last, _)| last.prefix != owner)
            {
                let generation = set
                    .generation()
                    .filter(|generated| !generated.snapshot.trim().is_empty());
                let base = match set.generation() {
                    Some(generated) => revision
                        .files(&PathBuf::from(generated.dir).join("migrations").join("sql"))?,
                    None => None,
                };
                directories.push((
                    Directory {
                        prefix: owner,
                        files: Vec::new(),
                        base,
                    },
                    generation.is_some(),
                ));
            }
            if let Some((directory, _)) = directories.last_mut() {
                directory
                    .files
                    .push((migration.name().to_owned(), migration.up().to_owned()));
            }
        }
        let own = self.disk_files(MigrationDatabase::SqlDb)?;
        let dir = self.manager().migration_directory(MigrationDatabase::SqlDb);
        directories.push((
            Directory {
                prefix: String::new(),
                files: own,
                base: revision.files(&dir)?,
            },
            false,
        ));
        Ok(directories)
    }

    /// The union of the committed snapshots of every SQL set generated from its models.
    fn committed_snapshots(&self) -> MigrationResult<Snapshot> {
        let mut union = Snapshot::default();
        for set in self.sql_sets() {
            let Some(generated) = set.generation() else {
                continue;
            };
            if generated.snapshot.trim().is_empty() {
                continue;
            }
            let snapshot =
                Snapshot::from_file(generated.snapshot).map_err(|error| MigrationError::Parse {
                    file: format!("{}/migrations/schema.json", generated.dir),
                    message: error.to_string(),
                })?;
            union.tables.extend(snapshot.tables);
        }
        Ok(union)
    }

    /// Every directory's files applied into `pool`'s schema, one directory at a time: the
    /// catalog read back, its unusable indexes, and the tables the directories no snapshot
    /// speaks for made.
    async fn replay(
        &self,
        pool: &SqlPool,
        directories: &[(Directory, bool)],
    ) -> MigrationResult<(Snapshot, Vec<SchemaDifference>, BTreeSet<String>)> {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut by_hand = BTreeSet::new();
        for (directory, generated) in directories {
            files.extend(
                directory
                    .files
                    .iter()
                    .map(|(name, text)| (format!("{}{name}", directory.prefix), text.clone())),
            );
            let before = table_names(pool).await?;
            self.apply_files(pool, &files).await?;
            if !generated {
                by_hand.extend(table_names(pool).await?.difference(&before).cloned());
            }
        }
        let (catalog, unusable) = catalog(pool).await?;
        Ok((catalog, unusable, by_hand))
    }

    /// The upgrade applied into `pool`'s schema: the files at the base, then the branch's.
    async fn upgrade(
        &self,
        pool: &SqlPool,
        plan: &Upgrade,
    ) -> MigrationResult<(Snapshot, Vec<SchemaDifference>)> {
        self.apply_files(pool, &plan.at_base).await?;
        let mut every = plan.at_base.clone();
        every.extend(plan.new.iter().cloned());
        self.apply_files(pool, &every).await?;
        catalog(pool).await
    }
}

/// The catalogs the two replays made.
struct Replays {
    scratch: Snapshot,
    scratch_unusable: Vec<SchemaDifference>,
    upgrade: Snapshot,
    upgrade_unusable: Vec<SchemaDifference>,
    /// The tables a directory no snapshot speaks for made.
    by_hand: BTreeSet<String>,
}

impl Replays {
    /// Both catalogs against the committed snapshots, the models where the scratch one is
    /// concerned, and each other.
    fn compare(self, committed: &Snapshot, models: &Snapshot) -> Report {
        let by_hand = &self.by_hand;
        let mut report = Report::default();
        report.section(
            "from scratch, against the committed snapshots",
            committed.differences(&kept(&self.scratch, committed, by_hand)),
        );
        report.section(
            "from scratch, against the models",
            models.differences(&kept(&self.scratch, models, by_hand)),
        );
        report.section("from scratch, in the catalog", self.scratch_unusable);
        report.section(
            "the upgrade, against the committed snapshots",
            committed.differences(&kept(&self.upgrade, committed, by_hand)),
        );
        report.section(
            "the upgrade, against the replay from scratch",
            self.scratch.differences(&self.upgrade),
        );
        report.section("the upgrade, in the catalog", self.upgrade_unusable);
        report
    }
}

/// `catalog` without the tables `expected` lacks that a directory no snapshot speaks for made.
fn kept(catalog: &Snapshot, expected: &Snapshot, by_hand: &BTreeSet<String>) -> Snapshot {
    Snapshot {
        tables: catalog
            .tables
            .iter()
            .filter(|(name, _)| expected.tables.contains_key(*name) || !by_hand.contains(*name))
            .map(|(name, table)| (name.clone(), table.clone()))
            .collect(),
        ..Snapshot::default()
    }
}

/// The catalog of `pool`'s schema as a snapshot, and each index in it whose build did not
/// finish.
pub(crate) async fn catalog(pool: &SqlPool) -> MigrationResult<(Snapshot, Vec<SchemaDifference>)> {
    let schema = SqlIntrospector::introspect(pool).await?;
    let snapshot = Snapshot::of(&schema.tables.into_values().collect::<Vec<_>>(), None);
    let unusable = SqlIntrospector::unusable_indexes(pool)
        .await
        .map_err(DatabaseError::from)?
        .into_iter()
        .map(|(table, index)| SchemaDifference {
            table,
            item: SchemaItem::Index(index),
            expected: Some("usable".to_owned()),
            found: Some("unusable: its build did not finish".to_owned()),
        })
        .collect();
    Ok((snapshot, unusable))
}

/// The tables in `pool`'s schema.
async fn table_names(pool: &SqlPool) -> MigrationResult<BTreeSet<String>> {
    let rows = pool
        .fetch_all(
            "SELECT table_name::text AS name FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_type = 'BASE TABLE'",
        )
        .await
        .map_err(DatabaseError::from)?;
    rows.iter()
        .map(|row| {
            row.try_get::<String, _>("name")
                .map_err(|error| MigrationError::Database(error.to_string()))
        })
        .collect()
}

/// What a verification found, under the comparison each difference came from.
#[derive(Default)]
struct Report {
    text: String,
    differences: Vec<SchemaDifference>,
}

impl Report {
    fn section(&mut self, title: &str, differences: Vec<SchemaDifference>) {
        if differences.is_empty() {
            return;
        }
        self.text.push_str(title);
        self.text.push_str(":\n");
        for difference in &differences {
            self.text.push_str("  ");
            self.text.push_str(&difference.to_string());
            self.text.push('\n');
        }
        self.differences.extend(differences);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use fundamental_db::migration::generate::{
        SchemaDifference, SchemaItem, Snapshot, SnapshotTable,
    };

    use super::{Report, Verification, kept};

    fn holding(names: &[&str]) -> Snapshot {
        Snapshot {
            tables: names
                .iter()
                .map(|name| ((*name).to_owned(), SnapshotTable::default()))
                .collect(),
            ..Snapshot::default()
        }
    }

    #[test]
    fn a_table_made_by_hand_is_compared_only_where_it_is_expected() {
        let catalog = holding(&["article", "lease", "stray"]);
        let by_hand = BTreeSet::from(["lease".to_owned()]);
        let kept_tables: Vec<String> = kept(&catalog, &holding(&["article"]), &by_hand)
            .tables
            .into_keys()
            .collect();
        assert_eq!(kept_tables, ["article", "stray"]);
        let kept_tables: Vec<String> = kept(&catalog, &holding(&["article", "lease"]), &by_hand)
            .tables
            .into_keys()
            .collect();
        assert_eq!(kept_tables, ["article", "lease", "stray"]);
    }

    #[test]
    fn a_report_names_each_difference_under_its_comparison() {
        let mut report = Report::default();
        report.section("from scratch, against the models", Vec::new());
        report.section(
            "the upgrade, against the replay from scratch",
            vec![SchemaDifference {
                table: "article".to_owned(),
                item: SchemaItem::Check("article_kind".to_owned()),
                expected: Some("kind IN ('news')".to_owned()),
                found: None,
            }],
        );
        assert_eq!(
            report.text,
            "the upgrade, against the replay from scratch:\n  article  check article_kind  \
             expected kind IN ('news')  found none\n"
        );
        assert_eq!(report.differences.len(), 1);
    }

    #[test]
    fn a_verification_s_summary_says_what_was_replayed() {
        let summary = Verification {
            base: "3953fc6a1b2c3d4e5f".to_owned(),
            from_scratch: 44,
            at_base: 41,
            branch: 3,
        }
        .summary();
        assert!(
            summary.starts_with(
                "from scratch 44 files; upgrade 41 files as at 3953fc6a1b2c, then 3 of this branch"
            ),
            "{summary}"
        );
    }
}
