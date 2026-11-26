//! `fun messages check`: every crate's messages, judged by the project's policy.

use std::path::{Path, PathBuf};
use std::process::Command;

use fundamental_messages_build::{Environment, Finding, Level, Policy, check};

use super::input::{MessagesArgs, MessagesCommand};
use crate::{CliError, CliResult, CommandContext};

/// The crates with a `locales/` folder beside their manifest, in name order; only `package` if named.
fn crates_with_messages(
    metadata: &serde_json::Value,
    package: Option<&str>,
) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|candidate| package.is_none_or(|wanted| candidate["name"] == wanted))
        .filter_map(|candidate| {
            let name = candidate["name"].as_str()?;
            let folder = Path::new(candidate["manifest_path"].as_str()?).parent()?;
            folder
                .join("locales")
                .is_dir()
                .then(|| (name.to_owned(), folder.to_path_buf()))
        })
        .collect();
    found.sort();
    found
}

fn judge(folder: &Path, environment: Environment) -> Result<Vec<Finding>, Vec<String>> {
    let policy = Policy::read(folder, environment).map_err(|problem| vec![problem])?;
    Ok(check(folder)?.findings(&policy))
}

/// Checks the messages of every crate in the workspace, or of the one named.
///
/// # Errors
/// [`CliError::MessagesUnreadable`] when they cannot be read; [`CliError::MessagesRefused`] for an error.
pub(crate) fn run(args: MessagesArgs, ctx: CommandContext) -> CliResult<()> {
    let MessagesCommand::Check { package, ci } = args.command;
    let environment = if ci {
        Environment::Ci
    } else {
        Environment::detect()
    };
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|error| CliError::MessagesUnreadable {
            package: "the workspace".to_owned(),
            problems: error.to_string(),
        })?;
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| CliError::MessagesUnreadable {
            package: "the workspace".to_owned(),
            problems: error.to_string(),
        })?;
    let ui = ctx.ui();
    ui.section("Messages (check)");
    let mut errors = 0;
    for (name, folder) in crates_with_messages(&metadata, package.as_deref()) {
        let findings =
            judge(&folder, environment).map_err(|problems| CliError::MessagesUnreadable {
                package: name.clone(),
                problems: problems.join("; "),
            })?;
        if findings.is_empty() {
            ui.detail(format!("{name}: {}", ui.ok("complete")));
            continue;
        }
        ui.detail(format!("{name}: {}", ui.warn(&findings.len().to_string())));
        for finding in &findings {
            ui.note(finding);
        }
        errors += findings
            .iter()
            .filter(|finding| finding.level == Level::Error)
            .count();
    }
    if errors > 0 {
        return Err(CliError::MessagesRefused { count: errors });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_crate_with_a_locales_folder_is_checked() {
        let root = tempfile::tempdir().expect("a folder");
        for (name, locales) in [("kept", true), ("plain", false)] {
            let folder = root.path().join(name);
            std::fs::create_dir_all(&folder).expect("the crate's folder");
            if locales {
                std::fs::create_dir_all(folder.join("locales/en")).expect("its locales");
            }
        }
        let manifest = |name: &str| {
            root.path()
                .join(name)
                .join("Cargo.toml")
                .to_string_lossy()
                .into_owned()
        };
        let metadata = serde_json::json!({ "packages": [
            { "name": "plain", "manifest_path": manifest("plain") },
            { "name": "kept", "manifest_path": manifest("kept") },
        ] });

        assert_eq!(
            crates_with_messages(&metadata, None),
            [("kept".to_owned(), root.path().join("kept"))]
        );
        assert!(crates_with_messages(&metadata, Some("plain")).is_empty());
    }
}
