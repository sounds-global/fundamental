//! Entry point for `fun run`: list the project's addresses, or dispatch one.
//!
//! Two sources claiming one address are refused, so a script never shadows a Rust command.

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use tracing::{debug, instrument};

use super::RunArgs;
use crate::emit::emit;
use crate::helpers::{
    CommandEntry, CommandIndex, CommandSource, Manifest, Project, resolve_app_bin,
};
use crate::{CliError, CliResult, CommandContext};

const ROLE: &str = "commands";

#[instrument(
    skip(ctx),
    fields(name = ?args.name, app = ?args.app, release = args.release),
    err(Debug)
)]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn run(args: RunArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let manifest = Manifest::load(&project.manifest_path())?;
    let index = CommandIndex::build(&project, &manifest)?;

    if args.list || args.name.is_none() {
        return list(&project, &index, &args, ctx);
    }
    let ui = ctx.ui();
    let address = args.name.clone().expect("checked above");
    let Some(entry) = index.get(&address) else {
        let suggestion = index
            .nearest(&address)
            .map(|near| format!(" Did you mean `{near}`?"))
            .unwrap_or_default();
        return Err(CliError::UnknownCommand {
            name: address,
            suggestion,
            count: index.len(),
        });
    };

    match &entry.source {
        CommandSource::Script { run, .. } => {
            debug!(command = %address, source = "manifest", "dispatching to a shell script");
            ui.section(format!("Run {address}"));
            ui.detail(run);
            run_shell(run, &args.args, project.root())
        }
        CommandSource::Rust { app, .. } => {
            // `--app` matters only for a second binary built from the same sources.
            let package = args.app.clone().unwrap_or_else(|| app.clone());
            debug!(command = %address, source = "rust", package = %package, "dispatching to the commands bin");
            let bin = resolve_bin(&project, &package, args.bin.as_deref())?;
            let cargo_args = cargo_args(
                Some(&package),
                &bin,
                Some(&address),
                &args.args,
                args.release,
            );
            ui.section(format!("Run {address}"));
            ui.detail(format!("cargo {}", cargo_args.join(" ")));
            cargo_run(&cargo_args)
        }
    }
}

/// List the project's addresses by namespace, building nothing; `args.name` filters to one.
fn list(
    project: &Project,
    index: &CommandIndex,
    args: &RunArgs,
    ctx: CommandContext,
) -> CliResult<()> {
    let ui = ctx.ui();
    let filter = args
        .name
        .as_deref()
        .map(|f| f.trim_end_matches(':').to_string());

    if args.json {
        // Through `emit`: a caller piping the answer must not get the accent bars.
        emit(&render_json(index, filter.as_deref())?);
        return Ok(());
    }

    let grouped = index.by_namespace();
    let shown: Vec<(&str, &Vec<&CommandEntry>)> = grouped
        .iter()
        .filter(|(namespace, _)| filter.as_deref().is_none_or(|f| **namespace == f))
        .map(|(namespace, entries)| (*namespace, entries))
        .collect();

    if shown.is_empty() {
        match &filter {
            Some(f) => ui.note(format!(
                "no namespace `{f}`; `fun run --list` shows all {}",
                grouped.len()
            )),
            None => ui.note("this project declares no commands"),
        }
        return Ok(());
    }

    let total: usize = shown.iter().map(|(_, e)| e.len()).sum();
    ui.section(format!(
        "fun run — {total} address{} in {} namespace{}",
        if total == 1 { "" } else { "es" },
        shown.len(),
        if shown.len() == 1 { "" } else { "s" }
    ));
    ui.blank();

    // The whole, copyable address, in one column width across namespaces (`CommandAddress` is ASCII).
    let addresses: Vec<String> = shown
        .iter()
        .flat_map(|(_, entries)| entries.iter())
        .map(|e| e.address.to_string())
        .collect();
    let width = addresses
        .iter()
        .map(|a| a.chars().count())
        .max()
        .unwrap_or(0);

    let room = description_room(width);

    for (index, (namespace, entries)) in shown.iter().enumerate() {
        if index > 0 {
            ui.blank();
        }
        ui.note(format!("{namespace} — {}", owners(entries)));
        for entry in *entries {
            let address = entry.address.to_string();
            let description = entry.description.as_deref().map(|d| clamp(d, room));
            ui.entry(&address, width, description.as_deref());
        }
    }

    if args.verify {
        return verify(project, index, args, &ctx);
    }
    Ok(())
}

/// Room for a description before the line wraps; `None` without a terminal, so nothing is cut.
fn description_room(width: usize) -> Option<usize> {
    if !std::io::stdout().is_terminal() {
        return None;
    }
    let (_, columns) = console::Term::stdout().size_checked()?;
    let columns = usize::from(columns);
    // 6 of indent + the address column + 2 of gap.
    columns.checked_sub(6 + width + 2).filter(|room| *room > 8)
}

fn clamp(text: &str, room: Option<usize>) -> String {
    let Some(room) = room else {
        return text.to_string();
    };
    if text.chars().count() <= room {
        return text.to_string();
    }
    let kept: String = text.chars().take(room.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

fn owners(entries: &[&CommandEntry]) -> String {
    let apps: BTreeSet<&str> = entries.iter().filter_map(|e| e.source.app()).collect();
    let files: BTreeSet<String> = entries
        .iter()
        .filter(|e| e.source.app().is_none())
        .map(|e| e.source.to_string())
        .collect();
    let mut parts: Vec<String> = files.into_iter().collect();
    parts.extend(apps.into_iter().map(str::to_string));
    parts.join(" · ")
}

/// The index as JSON.
///
/// # Errors
/// [`CliError::Serialize`] if the index cannot be rendered.
fn render_json(index: &CommandIndex, filter: Option<&str>) -> CliResult<String> {
    let rows: Vec<Value> = index
        .entries()
        .filter(|e| filter.is_none_or(|f| e.address.namespace() == f))
        .map(|entry| {
            json!({
                "address": entry.address_string(),
                "namespace": entry.address.namespace(),
                "name": entry.address.name(),
                "description": entry.description,
                "app": entry.source.app(),
            })
        })
        .collect();
    serde_json::to_string_pretty(&rows).map_err(|cause| CliError::Serialize {
        what: "the command index".to_string(),
        reason: cause.to_string(),
    })
}

/// Builds each binary and checks it registers exactly the addresses read from its sources.
fn verify(
    project: &Project,
    index: &CommandIndex,
    args: &RunArgs,
    ctx: &CommandContext,
) -> CliResult<()> {
    let ui = ctx.ui();
    let apps = project.apps_with_commands();
    ui.section(format!(
        "Verify — building {} commands binar{}",
        apps.len(),
        if apps.len() == 1 { "y" } else { "ies" }
    ));

    let mut drift = Vec::new();
    for app in &apps {
        let scanned: BTreeSet<String> = index.addresses_for_app(app).into_iter().collect();
        let bin = resolve_bin(project, app, args.bin.as_deref())?;
        let registered = registered_addresses(app, &bin, args.release)?;

        for missing in registered.difference(&scanned) {
            drift.push(format!(
                "{app}: `{missing}` is registered by the binary but was not read from the sources"
            ));
        }
        for missing in scanned.difference(&registered) {
            drift.push(format!(
                "{app}: `{missing}` was read from the sources but the binary does not register it \
                 — is it missing from `commands_main!`?"
            ));
        }
        ui.detail(format!("{app}: {} address(es)", registered.len()));
    }

    if drift.is_empty() {
        ui.status(format!(
            "the index matches every binary — {} address(es)",
            index.len()
        ));
        return Ok(());
    }
    for line in &drift {
        ui.note(line);
    }
    Err(CliError::CommandFailed { code: 1 })
}

fn registered_addresses(app: &str, bin: &str, release: bool) -> CliResult<BTreeSet<String>> {
    let cargo_args = cargo_args_list(Some(app), bin, release);
    let output = Command::new("cargo")
        .args(&cargo_args)
        .output()
        .map_err(|err| CliError::CommandSpawn {
            bin: "cargo".to_string(),
            reason: err.to_string(),
        })?;
    if !output.status.success() {
        return Err(CliError::CommandFailed {
            code: output.status.code().unwrap_or(1),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split('\t').next())
        .filter(|address| !address.is_empty())
        .map(str::to_string)
        .collect())
}

/// `--bin` if given, else `<app>-commands`, then `commands`.
///
/// # Errors
/// [`CliError::AppBinMissing`] when the app declares neither candidate.
fn resolve_bin(project: &Project, package: &str, bin: Option<&str>) -> CliResult<String> {
    match bin {
        Some(bin) => Ok(bin.to_string()),
        None => resolve_app_bin(project, package, ROLE),
    }
}

/// `run [-p <package>] [--release] --bin <bin> -- <address> [args…]`.
fn cargo_args(
    package: Option<&str>,
    bin: &str,
    address: Option<&str>,
    passthrough: &[String],
    release: bool,
) -> Vec<String> {
    let mut args = cargo_prefix(package, bin, release);
    if let Some(address) = address {
        args.push(address.to_string());
        args.extend(passthrough.iter().cloned());
    }
    args
}

/// `run [-p <package>] [--release] --bin <bin> -- --list`.
fn cargo_args_list(package: Option<&str>, bin: &str, release: bool) -> Vec<String> {
    let mut args = cargo_prefix(package, bin, release);
    args.push("--list".to_string());
    args
}

fn cargo_prefix(package: Option<&str>, bin: &str, release: bool) -> Vec<String> {
    let mut args = vec!["run".to_string()];
    if let Some(package) = package {
        args.push("-p".to_string());
        args.push(package.to_string());
    }
    if release {
        args.push("--release".to_string());
    }
    args.push("--bin".to_string());
    args.push(bin.to_string());
    args.push("--".to_string());
    args
}

fn cargo_run(cargo_args: &[String]) -> CliResult<()> {
    let status = Command::new("cargo")
        .args(cargo_args)
        .status()
        .map_err(|err| CliError::CommandSpawn {
            bin: "cargo".to_string(),
            reason: err.to_string(),
        })?;
    if !status.success() {
        return Err(CliError::CommandFailed {
            code: status.code().unwrap_or(1),
        });
    }
    Ok(())
}

/// Run a `[commands]` shell line from `cwd`, with any passthrough arguments appended.
fn run_shell(line: &str, passthrough: &[String], cwd: &Path) -> CliResult<()> {
    let mut command = shell_command(line, passthrough);
    command.current_dir(cwd);
    let status = command.status().map_err(|err| CliError::CommandSpawn {
        bin: SHELL.to_string(),
        reason: err.to_string(),
    })?;
    if !status.success() {
        return Err(CliError::CommandFailed {
            code: status.code().unwrap_or(1),
        });
    }
    Ok(())
}

#[cfg(not(windows))]
const SHELL: &str = "sh";
#[cfg(windows)]
const SHELL: &str = "cmd";

/// On Unix the arguments are `sh`'s positional parameters behind `"$@"`, so none is split or expanded.
#[cfg(not(windows))]
fn shell_command(line: &str, passthrough: &[String]) -> Command {
    let mut command = Command::new("sh");
    if passthrough.is_empty() {
        command.arg("-c").arg(line);
    } else {
        command
            .arg("-c")
            .arg(format!(r#"{line} "$@""#))
            .arg("fun")
            .args(passthrough);
    }
    command
}
#[cfg(windows)]
fn shell_command(line: &str, passthrough: &[String]) -> Command {
    let full = if passthrough.is_empty() {
        line.to_string()
    } else {
        format!("{line} {}", passthrough.join(" "))
    };
    let mut command = Command::new("cmd");
    command.arg("/C").arg(full);
    command
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_description_is_untouched_when_nothing_is_measured() {
        let long = "a".repeat(500);
        assert_eq!(clamp(&long, None), long);
    }

    #[test]
    fn a_description_that_fits_is_left_alone() {
        assert_eq!(clamp("short", Some(20)), "short");
        assert_eq!(clamp("12345", Some(5)), "12345");
    }

    #[test]
    fn a_cut_description_fits_the_room_including_its_ellipsis() {
        for room in 9..40usize {
            let cut = clamp("the quick brown fox jumps over the lazy dog", Some(room));
            assert!(
                cut.chars().count() <= room,
                "{room} columns of room produced {} characters: {cut:?}",
                cut.chars().count()
            );
            assert!(cut.ends_with('…'), "{cut:?} must say it was cut");
        }
    }

    #[test]
    fn a_cut_does_not_leave_a_space_before_its_ellipsis() {
        let cut = clamp("idempotent write", Some(12));
        assert!(!cut.contains(" …"), "{cut:?}");
    }

    use super::*;

    #[test]
    fn cargo_args_run_an_addressed_command_with_passthrough() {
        let args = cargo_args(
            Some("main"),
            "commands",
            Some("index:reindex"),
            &["--since".to_string(), "2020".to_string()],
            false,
        );
        assert_eq!(
            args,
            vec![
                "run",
                "-p",
                "main",
                "--bin",
                "commands",
                "--",
                "index:reindex",
                "--since",
                "2020"
            ]
        );
    }

    #[test]
    fn cargo_args_add_release_and_omit_package() {
        let args = cargo_args(None, "commands", Some("greet:hello"), &[], true);
        assert_eq!(
            args,
            vec!["run", "--release", "--bin", "commands", "--", "greet:hello"]
        );
    }

    #[test]
    fn cargo_args_list_targets_the_bin() {
        let args = cargo_args_list(Some("main"), "commands", false);
        assert_eq!(
            args,
            vec!["run", "-p", "main", "--bin", "commands", "--", "--list"]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn run_shell_succeeds_and_chains_with_and() {
        let tmp = tempfile::TempDir::new().unwrap();
        run_shell("true && true", &[], tmp.path()).expect("chained commands succeed");
    }

    #[cfg(not(windows))]
    #[test]
    fn run_shell_forwards_a_non_zero_exit_code() {
        let tmp = tempfile::TempDir::new().unwrap();
        let err = run_shell("exit 3", &[], tmp.path()).expect_err("non-zero exit");
        assert!(matches!(err, CliError::CommandFailed { code: 3 }));
    }

    #[cfg(not(windows))]
    #[test]
    fn run_shell_appends_passthrough_arguments() {
        let tmp = tempfile::TempDir::new().unwrap();
        run_shell("test ok =", &["ok".to_string()], tmp.path())
            .expect("the appended argument completes the shell line (exit 0)");
        let err = run_shell("test ok =", &["no".to_string()], tmp.path())
            .expect_err("a different appended argument makes the test fail");
        assert!(matches!(err, CliError::CommandFailed { code: 1 }));
    }

    #[cfg(not(windows))]
    #[test]
    fn run_shell_keeps_a_passthrough_argument_with_spaces_whole() {
        let tmp = tempfile::TempDir::new().unwrap();
        run_shell(r#"test "a b" ="#, &["a b".to_string()], tmp.path())
            .expect("the argument arrives as one word, spaces included");
        run_shell(r#"test '$HOME' ="#, &["$HOME".to_string()], tmp.path())
            .expect("the argument is never expanded by the shell");
    }

    #[cfg(not(windows))]
    #[test]
    fn run_shell_reports_a_spawn_failure_when_the_program_is_missing() {
        // A missing inner program exits 127 rather than failing to spawn.
        let tmp = tempfile::TempDir::new().unwrap();
        let err =
            run_shell("command-that-does-not-exist", &[], tmp.path()).expect_err("missing program");
        assert!(matches!(err, CliError::CommandFailed { .. }));
    }
}
