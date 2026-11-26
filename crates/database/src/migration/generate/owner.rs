//! An owner's generated migrations: checked against its models without a database, the next
//! file written, and a branch's files rebased on main's (`fun migrate rebase`).

use std::path::PathBuf;

use super::{Main, Rebased, Snapshot, UndeclaredChange, next_statements, rebase};
use crate::migration::ddl::{DdlStatement, recognise, recognise_all};
use crate::migration::lint::{Lint, LintFinding, judge, report, verdict};
use crate::migration::render::regenerated;
use crate::migration::{
    EmbeddedMigrations, Level, Lints, LintsError, MigrationDatabase, Retirements, Revision,
    content_hash, split_statements, statement_spans,
};

/// Why an owner's models and migrations do not agree, or the next file was not written.
#[derive(Debug)]
pub enum GenerateError {
    /// The set is not generated from its models.
    NotGenerated {
        /// The owner.
        owner: &'static str,
    },
    /// No snapshot was written yet: a set whose files were written by hand.
    NoSnapshot {
        /// The owner.
        owner: &'static str,
    },
    /// The snapshot is not the schema after the last file: a file changed beside the generator,
    /// or two branches each added one.
    Stale {
        /// The owner.
        owner: &'static str,
    },
    /// The models declare what no migration makes yet: these statements would.
    Pending {
        /// The owner.
        owner: &'static str,
        /// The statements of the next file.
        statements: Vec<String>,
    },
    /// The models change what the generator will not write without being told what it means.
    Refused {
        /// The owner.
        owner: &'static str,
        /// What, and what to declare.
        undeclared: Vec<UndeclaredChange>,
    },
    /// A file makes a role without looking it up in `pg_roles` first, which a migrator without
    /// `CREATEROLE` cannot run.
    RoleNotLookedUp {
        /// The owner.
        owner: &'static str,
        /// The file.
        migration: &'static str,
        /// The role it makes.
        role: String,
    },
    /// A file the owner says needs `CREATEROLE` makes no role without looking it up.
    NeedsNoCreaterole {
        /// The owner.
        owner: &'static str,
        /// The file.
        migration: &'static str,
    },
    /// A file drops or renames what the owner's models do not declare, and `drops_by_hand` does
    /// not name it.
    UndeclaredDrop {
        /// The owner.
        owner: &'static str,
        /// The file.
        migration: &'static str,
        /// The verb, such as `DROP COLUMN` or `RENAME COLUMN`.
        verb: String,
        /// What it names: `table.column` for a column or constraint.
        object: String,
    },
    /// A file the owner names in `drops_by_hand` drops nothing its models do not declare, or is
    /// not one of its files.
    DropsNothingByHand {
        /// The owner.
        owner: &'static str,
        /// The file.
        migration: &'static str,
    },
    /// The migration lints refuse the owner's files or the next file, or a model accepts a lint
    /// that fires nowhere.
    Linted {
        /// The owner.
        owner: &'static str,
        /// The findings.
        findings: Vec<LintFinding>,
    },
    /// Nothing differs, so there is no next file to write.
    NothingToWrite {
        /// The owner.
        owner: &'static str,
    },
    /// Main's file is not in the tree: main was not merged into this branch yet.
    MainFileMissing {
        /// The owner.
        owner: &'static str,
        /// Main's file.
        migration: String,
    },
    /// Main's file is in the tree, but not as main holds it.
    MainFileChanged {
        /// The owner.
        owner: &'static str,
        /// Main's file.
        migration: String,
    },
    /// A branch file the generator did not write: rebasing would lose it.
    WrittenByHand {
        /// The owner.
        owner: &'static str,
        /// The file.
        migration: String,
    },
    /// Main's migrations could not be read, or its snapshot is not the schema after them.
    MainUnreadable {
        /// The owner.
        owner: &'static str,
        /// What was wrong.
        detail: String,
    },
    /// The project's lint levels could not be read from its `fun.toml`.
    Lints(LintsError),
    /// The snapshot file is not a snapshot.
    Unreadable(serde_json::Error),
    /// A file could not be written.
    Io(std::io::Error),
}

impl std::fmt::Display for GenerateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotGenerated { owner } => {
                write!(
                    f,
                    "{owner}: its migrations are not generated from its models"
                )
            }
            Self::NoSnapshot { owner } => write!(
                f,
                "{owner}: no migrations/schema.json yet; once its schema test passes against a \
                 database, write it with `fun migrate generate baseline`"
            ),
            Self::Stale { owner } => write!(
                f,
                "{owner}: migrations/schema.json is not the schema after its last migration; \
                 if main gained a migration after this branch wrote its own, run `fun migrate \
                 rebase -p {owner}`"
            ),
            Self::Pending { owner, statements } => write!(
                f,
                "{owner}: the models declare what no migration makes yet; run `fun migrate \
                 generate <name>` to write:\n{}",
                statements.join("\n")
            ),
            Self::Refused { owner, undeclared } => {
                write!(
                    f,
                    "{owner}: the models change what is not generated without saying so:"
                )?;
                for change in undeclared {
                    write!(f, "\n  {}: {}", change.table, change.reason)?;
                }
                Ok(())
            }
            Self::RoleNotLookedUp { .. }
            | Self::NeedsNoCreaterole { .. }
            | Self::UndeclaredDrop { .. }
            | Self::DropsNothingByHand { .. } => self.fmt_file(f),
            Self::Linted { owner, findings } => write!(
                f,
                "{owner}: the migration lints refuse its files:\n{}",
                report(findings)
            ),
            Self::NothingToWrite { owner } => {
                write!(
                    f,
                    "{owner}: the models are their migrations; nothing to write"
                )
            }
            Self::MainFileMissing { .. }
            | Self::MainFileChanged { .. }
            | Self::WrittenByHand { .. }
            | Self::MainUnreadable { .. } => self.fmt_rebase(f),
            Self::Lints(error) => write!(f, "{error}"),
            Self::Unreadable(error) => {
                write!(f, "migrations/schema.json is not a snapshot: {error}")
            }
            Self::Io(error) => write!(f, "a migration file could not be written: {error}"),
        }
    }
}

impl std::error::Error for GenerateError {}

impl GenerateError {
    fn fmt_file(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RoleNotLookedUp {
                owner,
                migration,
                role,
            } => write!(
                f,
                "{owner}: {migration} makes the role {role} without looking it up in pg_roles, \
                 which a migrator without CREATEROLE cannot run; the generator writes `IF NOT \
                 EXISTS (SELECT FROM pg_roles WHERE rolname = '{role}') THEN CREATE ROLE …`, or \
                 say the file needs it: `migrations!(…, needs_createrole = [\"{migration}\"])`"
            ),
            Self::NeedsNoCreaterole { owner, migration } => write!(
                f,
                "{owner}: `needs_createrole` names {migration}, which makes no role without \
                 looking it up; take it out"
            ),
            Self::UndeclaredDrop {
                owner,
                migration,
                verb,
                object,
            } => write!(
                f,
                "{owner}: {migration} does {verb} {object}, which none of its models declares, \
                 and an app's migration guard refuses it on every start; retire it on the model \
                 (`#[model(retired(…))]`, or the owner's `retired = [\"…\"]` for a table), say \
                 the column it became `was` it, or name the file: \
                 `migrations!(…, drops_by_hand = [\"{migration}\"])`"
            ),
            Self::DropsNothingByHand { owner, migration } => write!(
                f,
                "{owner}: `drops_by_hand` names {migration}, which is not one of its files or \
                 drops nothing its models do not declare; take it out"
            ),
            _ => Ok(()),
        }
    }

    fn fmt_rebase(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MainFileMissing { owner, migration } => write!(
                f,
                "{owner}: main's {migration} is not in this tree; bring main in first (`git \
                 merge origin/main` or `git rebase origin/main`), then rebase"
            ),
            Self::MainFileChanged { owner, migration } => write!(
                f,
                "{owner}: main's {migration} is changed in this tree; take main's file as it \
                 is (`git checkout origin/main -- <its path>`), then rebase"
            ),
            Self::WrittenByHand { owner, migration } => write!(
                f,
                "{owner}: {migration} was not written by the generator, so rebasing would \
                 lose it; renumber it after main's last file by hand"
            ),
            Self::MainUnreadable { owner, detail } => {
                write!(f, "{owner}: main's migrations could not be read: {detail}")
            }
            _ => Ok(()),
        }
    }
}

fn last_hash(set: &EmbeddedMigrations) -> Option<String> {
    set.migrations()
        .last()
        .map(|migration| content_hash(migration.up()))
}

pub(super) const GENERATED_HEADER: &str = "-- Generated from the models by `fun migrate generate`.";

pub(super) fn generated_text(statements: &[String]) -> String {
    format!("{GENERATED_HEADER}\n\n{}\n", statements.join("\n\n"))
}

pub(super) fn lints_of(set: &EmbeddedMigrations) -> Result<Lints, GenerateError> {
    let generated = set
        .generation()
        .ok_or(GenerateError::NotGenerated { owner: set.owner() })?;
    Lints::read(generated.dir).map_err(GenerateError::Lints)
}

/// Refuses what the migration lints find in `set`'s files and `next`, under `lints`, as an
/// app's guard would, and each stale acceptance.
fn linted(
    set: &EmbeddedMigrations,
    lints: &Lints,
    next: Option<(&str, &str)>,
) -> Result<(), GenerateError> {
    let names: Vec<String> = set
        .migrations()
        .iter()
        .map(|migration| set.ledger_name(migration))
        .chain(next.map(|(name, _)| format!("{}/{name}", set.owner())))
        .collect();
    let files: Vec<(&str, &str)> = set
        .migrations()
        .iter()
        .map(|migration| migration.up())
        .chain(next.map(|(_, text)| text))
        .zip(&names)
        .map(|(up, name)| (name.as_str(), up))
        .collect();
    let retirements = Retirements::of([set]);
    let found = judge::findings(&files, lints, set.database() == MigrationDatabase::SqlDb);
    let schemas = set.schemas();
    let accepted: Vec<(&str, Lint)> = schemas
        .iter()
        .flat_map(|table| {
            table
                .accepted
                .keys()
                .map(|lint| (table.name.as_str(), *lint))
        })
        .collect();
    let mut findings = verdict::stale(accepted, &found);
    findings.extend(verdict::blocking(found, lints, |finding| {
        let migration = finding.migration.as_deref().unwrap_or_default();
        retirements.accepts(migration, finding.lint, &finding.table)
            || (finding.lint == Lint::Destructive
                && retirements.passes(migration, &finding.verb, &finding.object))
    }));
    if findings.is_empty() {
        Ok(())
    } else {
        Err(GenerateError::Linted {
            owner: set.owner(),
            findings,
        })
    }
}

pub(super) fn models(set: &EmbeddedMigrations, parent: Option<String>) -> Snapshot {
    Snapshot::of(&set.schemas(), parent).retiring(set.retired_tables())
}

/// The line git begins a conflict with.
const CONFLICT_MARKER: &str = "<<<<<<<";

fn shares_a_number(set: &EmbeddedMigrations) -> bool {
    let numbers: Vec<&str> = set
        .migrations()
        .iter()
        .filter_map(|migration| migration.name().split_once('_').map(|(number, _)| number))
        .collect();
    numbers.windows(2).any(|pair| pair[0] == pair[1])
}

fn committed(set: &EmbeddedMigrations) -> Result<Snapshot, GenerateError> {
    let generated = set
        .generation()
        .ok_or(GenerateError::NotGenerated { owner: set.owner() })?;
    if generated.snapshot.trim().is_empty() {
        return Err(GenerateError::NoSnapshot { owner: set.owner() });
    }
    if generated.snapshot.contains(CONFLICT_MARKER) {
        return Err(GenerateError::Stale { owner: set.owner() });
    }
    let snapshot = Snapshot::from_file(generated.snapshot).map_err(GenerateError::Unreadable)?;
    if snapshot == Snapshot::default() && !set.migrations().is_empty() {
        return Err(GenerateError::NoSnapshot { owner: set.owner() });
    }
    if snapshot.parent != last_hash(set) || shares_a_number(set) {
        return Err(GenerateError::Stale { owner: set.owner() });
    }
    Ok(snapshot)
}

/// Refuses a file of `set` that makes a role without looking it up, unless the owner says it
/// needs `CREATEROLE`, and a file so named that does not.
fn roles_looked_up(set: &EmbeddedMigrations) -> Result<(), GenerateError> {
    let unasked = |up: &str| {
        split_statements(up)
            .iter()
            .find_map(|statement| match recognise(statement) {
                Ok(DdlStatement::CreateRole {
                    role,
                    looked_up: false,
                }) => Some(role),
                _ => None,
            })
    };
    let said = set.files_needing_createrole();
    for migration in set.migrations() {
        let role = unasked(migration.up());
        match role {
            Some(role) if !said.contains(&migration.name()) => {
                return Err(GenerateError::RoleNotLookedUp {
                    owner: set.owner(),
                    migration: migration.name(),
                    role,
                });
            }
            None if said.contains(&migration.name()) => {
                return Err(GenerateError::NeedsNoCreaterole {
                    owner: set.owner(),
                    migration: migration.name(),
                });
            }
            _ => {}
        }
    }
    if let Some(migration) = said
        .iter()
        .find(|name| !set.migrations().iter().any(|m| m.name() == **name))
    {
        return Err(GenerateError::NeedsNoCreaterole {
            owner: set.owner(),
            migration,
        });
    }
    Ok(())
}

/// Refuses a destructive statement of `set`'s its models do not declare and `drops_by_hand`
/// does not name, and a file named there needlessly.
fn drops_declared(set: &EmbeddedMigrations) -> Result<(), GenerateError> {
    let retirements = Retirements::of([set]);
    let named = set.files_dropping_by_hand();
    for migration in set.migrations() {
        let undeclared = split_statements(migration.up())
            .iter()
            .filter_map(|statement| recognise_all(statement).ok())
            .flatten()
            .find_map(|recognised| match recognised {
                DdlStatement::Destructive { verb, object }
                    if !retirements.declared_by_models(set.owner(), &verb, &object) =>
                {
                    Some((verb, object))
                }
                _ => None,
            });
        match undeclared {
            Some((verb, object)) if !named.contains(&migration.name()) => {
                return Err(GenerateError::UndeclaredDrop {
                    owner: set.owner(),
                    migration: migration.name(),
                    verb,
                    object,
                });
            }
            None if named.contains(&migration.name()) => {
                return Err(GenerateError::DropsNothingByHand {
                    owner: set.owner(),
                    migration: migration.name(),
                });
            }
            _ => {}
        }
    }
    if let Some(migration) = named
        .iter()
        .find(|name| !set.migrations().iter().any(|m| m.name() == **name))
    {
        return Err(GenerateError::DropsNothingByHand {
            owner: set.owner(),
            migration,
        });
    }
    Ok(())
}

/// Whether `set`'s models are its migrations: the snapshot is current, nothing is pending,
/// and the role, drop and lint checks pass. Each owner runs it as a test.
///
/// # Errors
/// What disagrees: [`GenerateError::Pending`] with the statements the next file would hold.
pub fn check(set: &EmbeddedMigrations) -> Result<(), GenerateError> {
    let before = committed(set)?;
    let lints = lints_of(set)?;
    roles_looked_up(set)?;
    if lints.level(Lint::Destructive) == Level::Warn {
        drops_declared(set)?;
    }
    match next_statements(&before, &models(set, before.parent.clone()), &lints) {
        Ok(statements) if statements.is_empty() => linted(set, &lints, None),
        Ok(statements) => Err(GenerateError::Pending {
            owner: set.owner(),
            statements,
        }),
        Err(undeclared) => Err(GenerateError::Refused {
            owner: set.owner(),
            undeclared,
        }),
    }
}

/// Writes `set`'s next migration and the snapshot after it; answers the file written.
///
/// # Errors
/// [`GenerateError::NothingToWrite`] when the models are their migrations; a refusal, a stale
/// snapshot or a file that could not be written.
pub fn write_next(set: &EmbeddedMigrations, name: &str) -> Result<PathBuf, GenerateError> {
    let before = committed(set)?;
    let statements =
        next_statements(&before, &models(set, None), &lints_of(set)?).map_err(|undeclared| {
            GenerateError::Refused {
                owner: set.owner(),
                undeclared,
            }
        })?;
    if statements.is_empty() {
        return Err(GenerateError::NothingToWrite { owner: set.owner() });
    }
    let dir = migrations_dir(set)?;
    let text = generated_text(&statements);
    let next = format!("{:04}_{name}", set.migrations().len() + 1);
    linted(set, &lints_of(set)?, Some((&next, &text)))?;
    let file = dir
        .join("sql")
        .join(format!("{:04}_{name}.up.sql", set.migrations().len() + 1));
    std::fs::create_dir_all(dir.join("sql")).map_err(GenerateError::Io)?;
    std::fs::write(&file, &text).map_err(GenerateError::Io)?;
    let after = models(set, Some(content_hash(&text)));
    std::fs::write(dir.join("schema.json"), after.to_file()).map_err(GenerateError::Io)?;
    Ok(file)
}

/// Writes the first snapshot of a set whose migrations were written by hand.
///
/// # Errors
/// The set names no directory, or the file could not be written.
pub fn write_baseline(set: &EmbeddedMigrations) -> Result<PathBuf, GenerateError> {
    let file = migrations_dir(set)?.join("schema.json");
    std::fs::write(&file, models(set, last_hash(set)).to_file()).map_err(GenerateError::Io)?;
    Ok(file)
}

/// Rewrites `set`'s files as the generator writes them now, keeping every other statement,
/// comment and line; answers the files written.
///
/// A file's hash changes with it, so this is only for files no deployed database has run.
///
/// # Errors
/// A stale snapshot, a set not generated from its models, or a file that could not be written.
pub fn regenerate(set: &EmbeddedMigrations) -> Result<Vec<PathBuf>, GenerateError> {
    let before = committed(set)?;
    let sql = migrations_dir(set)?.join("sql");
    let last = set.migrations().last().map(|migration| migration.name());
    let mut written = Vec::new();
    for migration in set.migrations() {
        let up = migration.up();
        let text = rewritten(up);
        if text == up {
            continue;
        }
        let file = sql.join(format!("{}.up.sql", migration.name()));
        std::fs::create_dir_all(&sql).map_err(GenerateError::Io)?;
        std::fs::write(&file, &text).map_err(GenerateError::Io)?;
        if last == Some(migration.name()) {
            let after = Snapshot {
                parent: Some(content_hash(&text)),
                ..before.clone()
            };
            std::fs::write(sql.with_file_name("schema.json"), after.to_file())
                .map_err(GenerateError::Io)?;
        }
        written.push(file);
    }
    Ok(written)
}

/// `up` with each statement the generator wrote another way before written as it writes it
/// now.
fn rewritten(up: &str) -> String {
    let mut text = String::with_capacity(up.len());
    let mut kept = 0;
    for (span, statement) in statement_spans(up) {
        let now = regenerated(&statement).filter(|_| up[span.clone()] == statement);
        if let Some(now) = now {
            text.push_str(&up[kept..span.start]);
            text.push_str(&now);
            kept = span.end;
        }
    }
    text.push_str(&up[kept..]);
    text
}

pub(super) fn migrations_dir(set: &EmbeddedMigrations) -> Result<PathBuf, GenerateError> {
    let generated = set
        .generation()
        .ok_or(GenerateError::NotGenerated { owner: set.owner() })?;
    Ok(PathBuf::from(generated.dir).join("migrations"))
}

/// The variable `fun migrate generate <name>` sets for an owner's migration test: the next
/// file's name, or `baseline`.
pub const NAME_VARIABLE: &str = "FUNDAMENTAL_MIGRATION";

/// The variable `fun migrate regenerate` and `fun migrate rebase` set for an owner's migration
/// test, to `regenerate` or `rebase`.
pub const MODE_VARIABLE: &str = "FUNDAMENTAL_MIGRATION_MODE";

/// The variable `fun migrate rebase` sets for an owner's migration test: the base commit.
pub const BASE_VARIABLE: &str = "FUNDAMENTAL_MIGRATION_BASE";

/// What an owner's migration test does, from the variables `fun migrate` set for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Generation<'a> {
    /// Check that the models are the migrations.
    Check,
    /// Write the first snapshot of hand-written files.
    Baseline,
    /// Rewrite the files as the generator writes them now.
    Regenerate,
    /// Write the branch's files again on top of this commit's.
    Rebase(&'a str),
    /// Write the next file, of this name.
    Next(&'a str),
}

impl<'a> Generation<'a> {
    /// The mode `mode`, `name` and `base` ask for; an unknown mode, or `rebase` without a base,
    /// is refused.
    pub(crate) fn of(
        mode: Option<&str>,
        name: Option<&'a str>,
        base: Option<&'a str>,
    ) -> Result<Self, String> {
        match (mode.unwrap_or_default(), name.unwrap_or_default()) {
            ("regenerate", _) => Ok(Self::Regenerate),
            ("rebase", _) => match base.map(str::trim) {
                Some(base) if !base.is_empty() => Ok(Self::Rebase(base)),
                _ => Err(format!(
                    "{MODE_VARIABLE}=rebase needs {BASE_VARIABLE}, the commit to rebase on"
                )),
            },
            ("", "") => Ok(Self::Check),
            ("", "baseline") => Ok(Self::Baseline),
            ("", name) => Ok(Self::Next(name)),
            (mode, _) => Err(format!(
                "{MODE_VARIABLE}={mode} is no generation; it is `regenerate` or `rebase`"
            )),
        }
    }
}

/// `set`'s branch migrations rebased on the commit `base` names ([`rebase`]), read through git.
///
/// # Errors
/// [`GenerateError::MainUnreadable`] when `base` names no commit there, or what [`rebase`]
/// refuses.
pub(crate) fn rebase_on(set: &EmbeddedMigrations, base: &str) -> Result<Rebased, GenerateError> {
    let dir = migrations_dir(set)?;
    let revision =
        Revision::resolve(base, &dir).map_err(|error| GenerateError::MainUnreadable {
            owner: set.owner(),
            detail: error.to_string(),
        })?;
    rebase(set, &Main::read(&revision, set)?)
}

/// An owner's migration test: [`check`]s its models are its migrations, or, as the
/// [`NAME_VARIABLE`], [`MODE_VARIABLE`] and [`BASE_VARIABLE`] set by `fun migrate` ask, writes
/// the next file, the baseline, [`regenerate`]s or [`rebase`]s.
///
/// # Panics
/// With what disagrees, or what could not be written, so the test fails saying it.
pub fn the_models_are_their_migrations(set: &EmbeddedMigrations) {
    let mode = std::env::var(MODE_VARIABLE).ok();
    let name = std::env::var(NAME_VARIABLE).ok();
    let base = std::env::var(BASE_VARIABLE).ok();
    let generation = Generation::of(mode.as_deref(), name.as_deref(), base.as_deref())
        .unwrap_or_else(|refused| {
            panic!("{refused}");
        });
    let outcome = match generation {
        Generation::Check => check(set).map(|()| Vec::new()),
        Generation::Baseline => write_baseline(set).map(|file| vec![file]),
        Generation::Regenerate => regenerate(set),
        Generation::Rebase(base) => rebase_on(set, base).map(|rebased| {
            for file in &rebased.removed {
                println!("{}: removed {}", set.owner(), file.display());
            }
            rebased.written.into_iter().collect()
        }),
        Generation::Next(name) => write_next(set, name).map(|file| vec![file]),
    };
    match outcome {
        Ok(written) => {
            for file in written {
                println!("{}: wrote {}", set.owner(), file.display());
            }
        }
        Err(GenerateError::NothingToWrite { .. }) => {}
        Err(error) => panic!("{error}"),
    }
}
