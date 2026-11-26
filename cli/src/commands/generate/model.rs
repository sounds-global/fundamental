//! `fun generate model <NAME>`: scaffold a domain model and its empty migration pair.

use std::path::PathBuf;

use tracing::{info, instrument};

use super::model_template::model_source;
use super::{Field, GenerateModelArgs, ModelName};
use crate::commands::migrate::migration_timestamp;
use crate::engine::Engine;
use crate::helpers::{CrateManifest, Project, ensure_module_decl, write_file};
use crate::helpers::{down_stub, up_stub};
use crate::ui::Ui;
use crate::{CliResult, CommandContext};

/// Where a generated model lives: a shared `domain/` crate or a single app.
enum ModelCrate {
    Domain { dir: PathBuf },
    App { dir: PathBuf },
}

impl ModelCrate {
    fn src_dir(&self) -> PathBuf {
        self.dir().join("src")
    }

    fn dir(&self) -> &PathBuf {
        match self {
            ModelCrate::Domain { dir } | ModelCrate::App { dir } => dir,
        }
    }

    fn entry_file(&self) -> PathBuf {
        match self {
            ModelCrate::Domain { .. } => self.src_dir().join("lib.rs"),
            ModelCrate::App { .. } => self.src_dir().join("main.rs"),
        }
    }

    /// `pub` for a domain crate's models, private for an app's.
    fn models_decl(&self) -> &'static str {
        match self {
            ModelCrate::Domain { .. } => "pub mod models;",
            ModelCrate::App { .. } => "mod models;",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            ModelCrate::Domain { .. } => "domain",
            ModelCrate::App { .. } => "app",
        }
    }
}

/// Write the model, wire it into its crate, amend `Cargo.toml`, and write an empty migration pair.
///
/// # Errors
/// A [`CliError`](crate::CliError) for an invalid name or field, no project, or an unwritable file.
#[instrument(skip(args, ctx), fields(name = %args.name, db = %args.db, app = %args.app), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn run(args: GenerateModelArgs, ctx: CommandContext) -> CliResult<()> {
    let name = ModelName::parse(&args.name)?;
    let fields = args
        .fields
        .iter()
        .map(|spec| Field::parse(spec))
        .collect::<Result<Vec<_>, _>>()?;
    let engine = Engine::parse(&args.db);

    let project = Project::discover()?;
    let model_crate = resolve_model_crate(&project, &args.app, args.crate_name.as_deref())?;

    let model_path = model_crate
        .src_dir()
        .join("models")
        .join(format!("{}.rs", name.snake()));
    write_file(&model_path, &model_source(&name, &fields, &engine))?;

    let models_mod = model_crate.src_dir().join("models").join("mod.rs");
    ensure_module_decl(&models_mod, &format!("pub mod {};", name.snake()))?;
    ensure_module_decl(&model_crate.entry_file(), model_crate.models_decl())?;

    amend_cargo_toml(&model_crate, &engine)?;

    let (up_rel, down_rel, stem) =
        write_migration(&project, &args.app, &model_crate, &name, &engine)?;

    info!(
        model = name.type_name(),
        table = name.snake(),
        "generated model"
    );
    let model_rel = relative_to(project.root(), &model_path);
    print_success(
        ctx.ui(),
        &name,
        &model_crate,
        &model_rel,
        &up_rel,
        &down_rel,
        &stem,
    );
    Ok(())
}

/// A domain crate when `domain/` exists (resolved or asked for), else the named app.
///
/// # Errors
/// [`CliError`](crate::CliError) when `domain/` holds no crate, or several and none was named.
fn resolve_model_crate(
    project: &Project,
    app: &str,
    wanted: Option<&str>,
) -> CliResult<ModelCrate> {
    if project.domain_dir().is_dir() {
        return Ok(ModelCrate::Domain {
            dir: project.resolve_domain_crate(wanted)?,
        });
    }
    Ok(ModelCrate::App {
        dir: project.app_dir(app),
    })
}

/// Add `uuid` and `serde`, and the engine: a capability feature on a domain crate, direct on an app.
///
/// # Errors
/// A [`CliError`](crate::CliError) if the `Cargo.toml` cannot be read, parsed, edited or written.
fn amend_cargo_toml(model_crate: &ModelCrate, engine: &Engine) -> CliResult<()> {
    let cargo_path = model_crate.dir().join("Cargo.toml");
    let mut manifest = CrateManifest::load(&cargo_path)?;
    // `js` gives `uuid`'s `v4` randomness on `wasm32`, where the model also compiles.
    manifest.ensure_dependency("uuid", "1", &["v4", "js"]);
    manifest.ensure_dependency("serde", "1", &["derive"]);
    match model_crate {
        ModelCrate::Domain { .. } => {
            manifest.ensure_engine_capability(engine.feature());
        }
        ModelCrate::App { .. } => {
            manifest.ensure_fundamental_feature(engine.feature())?;
        }
    }
    manifest.save()
}

/// Write the empty migration pair; answers the directory, both relative paths and the stem.
///
/// # Errors
/// A [`CliError`](crate::CliError) if either migration file cannot be written.
fn write_migration(
    project: &Project,
    app: &str,
    model_crate: &ModelCrate,
    name: &ModelName,
    engine: &Engine,
) -> CliResult<(String, String, String)> {
    let base = match model_crate {
        ModelCrate::App { .. } => project.app_dir(app),
        ModelCrate::Domain { dir } => dir.clone(),
    };
    let engine_dir = base.join("migrations").join(engine.dir_name());

    let ext = engine.extension();
    let stem = format!("{}_create_{}", migration_timestamp(), name.snake());
    let up = engine_dir.join(format!("{stem}.up.{ext}"));
    let down = engine_dir.join(format!("{stem}.down.{ext}"));

    // A stub: a schema is not a struct, and DDL from the fields could not express sharding or indexes.
    write_file(&up, up_stub())?;
    write_file(&down, down_stub())?;

    let rel = |p: &PathBuf| relative_to(project.root(), p);
    Ok((rel(&up), rel(&down), stem))
}

fn relative_to(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[expect(clippy::too_many_arguments, reason = "readability baseline")]
fn print_success(
    ui: &Ui,
    name: &ModelName,
    model_crate: &ModelCrate,
    model_rel: &str,
    up_rel: &str,
    down_rel: &str,
    stem: &str,
) {
    ui.blank();
    ui.section(format!(
        "Generated model {} ({} crate)",
        name.type_name(),
        model_crate.label()
    ));
    ui.detail(model_rel);
    ui.detail(up_rel);
    ui.detail(down_rel);
    ui.blank();
    ui.section("Next steps");
    ui.note(format!(
        "review the generated migration `{stem}` and its column types"
    ));
    ui.command("cargo build");
    ui.blank();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_maps_backend_to_fundamental_feature() {
        assert_eq!(Engine::SqlDb.feature(), "sql");
        assert_eq!(Engine::CqlDb.feature(), "cql");
        assert_eq!(Engine::Other("duckdb".into()).feature(), "sql");
    }

    #[test]
    fn domain_crate_uses_a_pub_models_decl_and_lib_entry() {
        let mc = ModelCrate::Domain {
            dir: PathBuf::from("/x/domain"),
        };
        assert_eq!(mc.models_decl(), "pub mod models;");
        assert!(mc.entry_file().ends_with("src/lib.rs"));
        assert_eq!(mc.label(), "domain");
    }

    #[test]
    fn app_crate_uses_a_private_models_decl_and_main_entry() {
        let mc = ModelCrate::App {
            dir: PathBuf::from("/x/apps/main"),
        };
        assert_eq!(mc.models_decl(), "mod models;");
        assert!(mc.entry_file().ends_with("src/main.rs"));
        assert_eq!(mc.label(), "app");
    }
}
