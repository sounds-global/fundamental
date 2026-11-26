//! `fun generate slice <NAME>`: a domain slice and its syncable base model.

use std::fs;
use std::path::Path;

use dialoguer::Select;
use tracing::{info, instrument};

use super::slice_template::slice_source;
use super::{GenerateSliceArgs, ModelName};
use crate::engine::Engine;
use crate::helpers::{CrateManifest, Manifest, Project, ensure_module_decl, write_file};
use crate::ui::Ui;
use crate::{CliError, CliResult, CommandContext};

/// Write the slice into `domain/src/sync/`, wire the gated `sync` module, and the feature matrix.
///
/// # Errors
/// A [`CliError`] for an invalid name, no project, or an unwritable file.
#[instrument(skip(args, ctx), fields(name = %args.name), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn run(args: GenerateSliceArgs, ctx: CommandContext) -> CliResult<()> {
    let name = ModelName::parse(&args.name)?;
    let snake = name.snake().to_string();
    let pascal = pascalize(&snake);

    let project = Project::discover()?;
    let domain = project.resolve_domain_crate(args.crate_name.as_deref())?;
    let src = domain.join("src");

    let engine = resolve_engine(args.db.as_deref(), &project, ctx.ui())?;

    let slice_path = src.join("sync").join(format!("{snake}.rs"));
    write_file(&slice_path, &slice_source(&snake, &pascal, &engine))?;

    let sync_mod = src.join("sync").join("mod.rs");
    ensure_module_decl(&sync_mod, &format!("mod {snake};"))?;
    ensure_module_decl(&sync_mod, &format!("pub use {snake}::*;"))?;
    ensure_gated_sync_module(&src.join("lib.rs"))?;

    amend_cargo_toml(&domain.join("Cargo.toml"), &engine)?;

    info!(slice = %snake, engine = engine.feature(), "generated slice");
    print_success(ctx.ui(), &snake, &pascal, &engine);
    Ok(())
}

/// `--db`, else the sole wired backend, YSQL with none, or a prompt (when attended) among several.
///
/// # Errors
/// [`CliError::InvalidDatabase`], [`CliError::AmbiguousDatabase`], or a manifest or prompt error.
fn resolve_engine(db: Option<&str>, project: &Project, ui: &Ui) -> CliResult<Engine> {
    if let Some(value) = db {
        return parse_engine_flag(value);
    }
    let engines = Manifest::load(&project.manifest_path())?.database_engines();
    match engines.as_slice() {
        [] => {
            ui.note(
                "no database in fun.toml; defaulting the slice model to SqlDb (pass --db cql to override)",
            );
            Ok(Engine::SqlDb)
        }
        [only] => Ok(only.clone()),
        many if console::user_attended() => prompt_engine(many),
        many => Err(CliError::AmbiguousDatabase {
            engines: engine_list(many),
        }),
    }
}

fn parse_engine_flag(value: &str) -> CliResult<Engine> {
    match value.trim().to_ascii_lowercase().as_str() {
        "sql" => Ok(Engine::SqlDb),
        "cql" => Ok(Engine::CqlDb),
        _ => Err(CliError::InvalidDatabase {
            value: value.to_string(),
        }),
    }
}

fn prompt_engine(engines: &[Engine]) -> CliResult<Engine> {
    let labels: Vec<&str> = engines.iter().map(engine_label).collect();
    let index = Select::with_theme(&Ui::prompt_theme())
        .with_prompt("Which database should the slice model target?")
        .items(&labels)
        .default(0)
        .interact()
        .map_err(|e| CliError::PromptFailed {
            reason: format!("prompt failed: {e}"),
        })?;
    Ok(engines[index].clone())
}

fn engine_label(engine: &Engine) -> &str {
    match engine {
        Engine::SqlDb => "sql",
        Engine::CqlDb => "cql",
        Engine::Other(name) => name,
    }
}

fn engine_list(engines: &[Engine]) -> String {
    engines
        .iter()
        .map(engine_label)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The declaration is two lines (`cfg` and `mod`), so `ensure_module_decl` cannot match it.
fn ensure_gated_sync_module(entry: &Path) -> CliResult<()> {
    let existing = match fs::read_to_string(entry) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(CliError::Io {
                path: entry.display().to_string(),
                reason: e.to_string(),
            });
        }
    };
    if existing.lines().any(|line| line.trim() == "pub mod sync;") {
        return Ok(());
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str("#[cfg(feature = \"sync\")]\npub mod sync;\n");
    fs::write(entry, next).map_err(|e| CliError::Io {
        path: entry.display().to_string(),
        reason: e.to_string(),
    })
}

/// The sync feature matrix (`device`, `sync-server`), the engine capability, and `uuid`/`serde`.
fn amend_cargo_toml(cargo_path: &Path, engine: &Engine) -> CliResult<()> {
    let mut manifest = CrateManifest::load(cargo_path)?;
    manifest.ensure_dependency("uuid", "1", &["v4", "js"]);
    manifest.ensure_dependency("serde", "1", &["derive"]);
    manifest.ensure_crate_feature("sync", "fundamental/sync");
    manifest.ensure_crate_feature("device", "sync");
    manifest.ensure_crate_feature("device", "fundamental/ws_client");
    manifest.ensure_crate_feature("sync-server", "sync");
    manifest.ensure_crate_feature("sync-server", "fundamental/sync_server");
    manifest.ensure_engine_capability(engine.feature());
    manifest.save()
}

fn pascalize(snake: &str) -> String {
    snake
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn print_success(ui: &Ui, snake: &str, pascal: &str, engine: &Engine) {
    ui.blank();
    ui.section(format!(
        "Generated slice {snake} ({} model, domain crate)",
        engine_label(engine)
    ));
    ui.detail(format!("domain/src/sync/{snake}.rs"));
    ui.blank();
    ui.section("Next steps");
    ui.note("serve it — list its marker type in a sync surface's `mount_slices!`:");
    ui.code(format!("mount_slices! {{ domain::sync::{pascal} }}"));
    ui.note("bind it on a device — `mount_slices!` generates the typed binder:");
    ui.code(format!("slices::{snake}(local_table, params, strategy)?"));
    ui.note("pull-only by default; add `#[model(bidirectional)]` to accept device writes");
    ui.blank();
    ui.command("cargo build");
    ui.blank();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascalize_handles_single_and_multiword_names() {
        assert_eq!(pascalize("mine"), "Mine");
        assert_eq!(pascalize("my_channel"), "MyChannel");
    }

    #[test]
    fn parse_engine_flag_accepts_the_two_backends_case_insensitively() {
        assert_eq!(parse_engine_flag("sql").unwrap(), Engine::SqlDb);
        assert_eq!(parse_engine_flag(" CQL ").unwrap(), Engine::CqlDb);
    }

    #[test]
    fn parse_engine_flag_rejects_an_unknown_backend() {
        use fundamental::core::Error as _;
        let err = parse_engine_flag("duckdb").expect_err("unknown backend");
        assert!(matches!(err, CliError::InvalidDatabase { .. }));
        assert_eq!(err.error_code(), "CLI_016");
    }

    #[test]
    fn engine_list_joins_labels_for_the_ambiguous_error() {
        let list = engine_list(&[Engine::SqlDb, Engine::CqlDb]);
        assert_eq!(list, "sql, cql");
    }
}
