//! `fun migrate generate`, `regenerate` and `rebase`, run in each owner's `migrations` test.

use std::process::Command;

use crate::{CliError, CliResult, CommandContext};

/// Read by `fundamental_db::migration::generate::NAME_VARIABLE`.
const NAME_VARIABLE: &str = "FUNDAMENTAL_MIGRATION";

/// Read by `fundamental_db::migration::generate::MODE_VARIABLE`.
const MODE_VARIABLE: &str = "FUNDAMENTAL_MIGRATION_MODE";

/// Read by `fundamental_db::migration::generate::BASE_VARIABLE`.
const BASE_VARIABLE: &str = "FUNDAMENTAL_MIGRATION_BASE";

/// What the owners' migration tests are run to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Writes {
    /// The next file, of this name (`baseline`: the first snapshot of hand-written files).
    Next(String),
    /// Every file again, as the generator writes it now.
    Regenerate,
    /// The branch's files again, on top of this commit's.
    Rebase(String),
}

impl Writes {
    /// The variables to set and those to clear, so one left in the shell never asks for another generation.
    fn variables(&self) -> (Vec<(&'static str, &str)>, &'static [&'static str]) {
        match self {
            Self::Next(name) => (vec![(NAME_VARIABLE, name)], &[MODE_VARIABLE, BASE_VARIABLE]),
            Self::Regenerate => (
                vec![(MODE_VARIABLE, "regenerate")],
                &[NAME_VARIABLE, BASE_VARIABLE],
            ),
            Self::Rebase(commit) => (
                vec![(MODE_VARIABLE, "rebase"), (BASE_VARIABLE, commit)],
                &[NAME_VARIABLE],
            ),
        }
    }
}

/// A package with a `migrations` test, and the features that test requires.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Owner {
    package: String,
    features: Vec<String>,
}

/// The owners in `metadata`, or only `package`; unread, every owner, so `cargo` says what is missing.
fn owners(metadata: &serde_json::Value, package: Option<&str>) -> Vec<Owner> {
    let read: Vec<Owner> = metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|candidate| package.is_none_or(|package| candidate["name"] == package))
        .filter_map(|candidate| {
            let test = candidate["targets"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|target| target["name"] == "migrations")?;
            Some(Owner {
                package: candidate["name"].as_str()?.to_owned(),
                features: test["required-features"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|feature| feature.as_str().map(str::to_owned))
                    .collect(),
            })
        })
        .collect();
    match (read.is_empty(), package) {
        (true, Some(package)) => vec![Owner {
            package: package.to_owned(),
            features: Vec::new(),
        }],
        _ => read,
    }
}

/// Nothing when it cannot be read.
fn workspace_metadata() -> serde_json::Value {
    Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .ok()
        .and_then(|output| serde_json::from_slice(&output.stdout).ok())
        .unwrap_or_default()
}

/// The `cargo` arguments running every owner's migration test, with its features, in one build.
fn cargo_args(owners: &[Owner]) -> Vec<String> {
    let mut args = vec!["test".to_owned()];
    if owners.is_empty() {
        args.push("--workspace".to_owned());
    }
    for owner in owners {
        args.push("-p".to_owned());
        args.push(owner.package.clone());
    }
    let features: Vec<String> = owners
        .iter()
        .flat_map(|owner| {
            owner
                .features
                .iter()
                .map(|feature| format!("{}/{feature}", owner.package))
        })
        .collect();
    if !features.is_empty() {
        args.push("--features".to_owned());
        args.push(features.join(","));
    }
    args.extend(
        [
            "--test",
            "migrations",
            "--",
            "--exact",
            "the_models_are_their_migrations",
            "--nocapture",
        ]
        .map(str::to_owned),
    );
    args
}

/// Runs the owners' migration tests, set to write what `writes` names.
///
/// # Errors
/// [`CliError::MigrateSpawn`] when `cargo` cannot start; [`CliError::MigrateFailed`] when an owner refuses.
pub(crate) fn run(package: Option<String>, writes: &Writes, ctx: CommandContext) -> CliResult<()> {
    let cargo = cargo_args(&owners(&workspace_metadata(), package.as_deref()));
    let (set, cleared) = writes.variables();
    let ui = ctx.ui();
    ui.section(match writes {
        Writes::Next(_) => "Migrate (generate)",
        Writes::Regenerate => "Migrate (regenerate)",
        Writes::Rebase(_) => "Migrate (rebase)",
    });
    let shown: Vec<String> = set
        .iter()
        .map(|(variable, value)| format!("{variable}={value}"))
        .collect();
    ui.detail(format!("{} cargo {}", shown.join(" "), cargo.join(" ")));
    let mut command = Command::new("cargo");
    command.args(&cargo).envs(set);
    for variable in cleared {
        command.env_remove(variable);
    }
    let status = command.status().map_err(|err| CliError::MigrateSpawn {
        bin: "cargo test".to_owned(),
        reason: err.to_string(),
    })?;
    if !status.success() {
        return Err(CliError::MigrateFailed {
            code: status.code().unwrap_or(1),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Writes, cargo_args, owners};

    #[test]
    fn a_rewrite_is_asked_for_in_its_own_variable_and_a_name_in_the_names() {
        assert_eq!(
            Writes::Regenerate.variables(),
            (
                vec![("FUNDAMENTAL_MIGRATION_MODE", "regenerate")],
                &["FUNDAMENTAL_MIGRATION", "FUNDAMENTAL_MIGRATION_BASE"][..]
            )
        );
        assert_eq!(
            Writes::Next("regenerate".to_owned()).variables(),
            (
                vec![("FUNDAMENTAL_MIGRATION", "regenerate")],
                &["FUNDAMENTAL_MIGRATION_MODE", "FUNDAMENTAL_MIGRATION_BASE"][..]
            )
        );
    }

    #[test]
    fn a_rebase_names_its_mode_and_the_commit_and_clears_the_name() {
        assert_eq!(
            Writes::Rebase("3953fc6".to_owned()).variables(),
            (
                vec![
                    ("FUNDAMENTAL_MIGRATION_MODE", "rebase"),
                    ("FUNDAMENTAL_MIGRATION_BASE", "3953fc6")
                ],
                &["FUNDAMENTAL_MIGRATION"][..]
            )
        );
    }

    fn workspace() -> serde_json::Value {
        serde_json::json!({"packages": [
            {"name": "newsroom", "targets": [
                {"name": "migrations", "kind": ["test"], "required-features": []}
            ]},
            {"name": "brand", "targets": [
                {"name": "brand", "kind": ["lib"], "required-features": []}
            ]},
            {"name": "portfolio", "targets": [
                {"name": "portfolio", "kind": ["lib"], "required-features": []},
                {"name": "migrations", "kind": ["test"], "required-features": ["records"]}
            ]}
        ]})
    }

    const TEST: &str = "--test migrations -- --exact the_models_are_their_migrations --nocapture";

    #[test]
    fn every_owner_is_run_with_the_features_its_migration_test_needs() {
        assert_eq!(
            cargo_args(&owners(&workspace(), None)).join(" "),
            format!("test -p newsroom -p portfolio --features portfolio/records {TEST}")
        );
    }

    #[test]
    fn one_owner_is_run_with_the_features_its_migration_test_needs() {
        assert_eq!(
            cargo_args(&owners(&workspace(), Some("portfolio"))).join(" "),
            format!("test -p portfolio --features portfolio/records {TEST}")
        );
        assert_eq!(
            cargo_args(&owners(&workspace(), Some("newsroom"))).join(" "),
            format!("test -p newsroom {TEST}")
        );
    }

    #[test]
    fn an_owner_or_a_workspace_not_read_is_left_to_cargo_to_refuse() {
        assert_eq!(
            cargo_args(&owners(&workspace(), Some("absent"))).join(" "),
            format!("test -p absent {TEST}")
        );
        assert_eq!(
            cargo_args(&owners(&serde_json::Value::Null, None)).join(" "),
            format!("test --workspace {TEST}")
        );
    }
}
