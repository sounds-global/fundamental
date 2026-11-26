//! `fun generate action <NAME>`: a domain action and its request/response types.

use tracing::{info, instrument};

use super::action_template::action_source;
use super::{GenerateActionArgs, ModelName};
use crate::helpers::{CrateManifest, Project, ensure_module_decl, write_file};
use crate::ui::Ui;
use crate::{CliResult, CommandContext};

/// Write the action into `domain/src/actions/`, wire its re-exports, and ensure `serde`.
///
/// # Errors
/// A [`CliError`](crate::CliError) for an invalid name, no project, or an unwritable file.
#[instrument(skip(args, ctx), fields(name = %args.name), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn run(args: GenerateActionArgs, ctx: CommandContext) -> CliResult<()> {
    let name = ModelName::parse(&args.name)?;
    let snake = name.snake().to_string();
    let pascal = pascalize(&snake);

    let project = Project::discover()?;
    let domain = project.resolve_domain_crate(args.crate_name.as_deref())?;
    let src = domain.join("src");

    let action_path = src.join("actions").join(format!("{snake}.rs"));
    write_file(&action_path, &action_source(&snake, &pascal))?;

    // The glob re-export lets a browser stub name the DTOs as `domain::{Pascal}Request`.
    let actions_mod = src.join("actions").join("mod.rs");
    ensure_module_decl(&actions_mod, &format!("mod {snake};"))?;
    ensure_module_decl(&actions_mod, &format!("pub use {snake}::*;"))?;
    let lib = src.join("lib.rs");
    ensure_module_decl(&lib, "pub mod actions;")?;
    ensure_module_decl(&lib, "pub use actions::*;")?;

    let cargo = domain.join("Cargo.toml");
    let mut manifest = CrateManifest::load(&cargo)?;
    manifest.ensure_dependency("serde", "1", &["derive"]);
    manifest.save()?;

    info!(action = %snake, path = %format!("/api/{snake}"), "generated action");
    print_success(ctx.ui(), &snake, &pascal);
    Ok(())
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

fn print_success(ui: &Ui, snake: &str, pascal: &str) {
    ui.blank();
    ui.section(format!("Generated action {snake} (domain crate)"));
    ui.detail(format!("domain/src/actions/{snake}.rs"));
    ui.blank();
    ui.section("Next steps");
    ui.note(
        "serve it — list its marker type in a surface's `mount_actions!` (`main.rs`/`lib.rs`):",
    );
    ui.code(format!("mount_actions! {{ domain::{pascal} }}"));
    ui.note("call it from an island — `mount_actions!` generates the typed stub:");
    ui.code(format!("actions::{snake}(req).await"));
    ui.blank();
    ui.command("cargo build");
    ui.blank();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascalize_handles_single_and_multiword_names() {
        assert_eq!(pascalize("greet"), "Greet");
        assert_eq!(pascalize("create_user"), "CreateUser");
        assert_eq!(pascalize("a_b_c"), "ABC");
    }
}
