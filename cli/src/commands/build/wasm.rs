//! `fun build wasm`: a `#[wasm_bindgen]` crate built into a scoped npm package with `wasm-pack`.

use std::path::Path;
use std::process::Command;

use clap::Args;

use crate::helpers::Project;
use crate::ui::Ui;
use crate::{CliError, CliResult};

#[derive(Debug, Args)]
pub struct WasmArgs {
    /// The library crate's directory relative to the project root (e.g. `crates/acme_wallet_wasm`)
    #[arg(value_name = "CRATE_PATH")]
    crate_path: String,

    /// The `wasm-pack` target: `web`, `bundler`, `nodejs`, `no-modules`, or `deno`
    #[arg(long, default_value = "web")]
    target: String,

    /// The npm scope, without the leading `@` (e.g. `acme`)
    #[arg(long)]
    scope: Option<String>,

    /// The package name written into `pkg/package.json` (e.g. `@acme/wallet`); default the crate's
    #[arg(long)]
    npm_name: Option<String>,

    /// The `wasm-pack --out-name`, the js/wasm basename (e.g. `wallet`)
    #[arg(long)]
    out_name: Option<String>,

    /// Build with the unoptimized dev profile (`wasm-pack` defaults to release)
    #[arg(long)]
    dev: bool,
}

/// Execute `fun build wasm`: run `wasm-pack`, then rewrite the package name.
///
/// # Errors
/// [`CliError::Io`] if `wasm-pack` cannot be launched or `package.json` read or written;
/// [`CliError::BuildFailed`] if `wasm-pack` exits non-zero.
pub(crate) fn run(args: WasmArgs, project: &Project, ui: &Ui) -> CliResult<()> {
    let crate_path = project.root().join(&args.crate_path);
    ui.section(format!("Building wasm package from {}", args.crate_path));
    ui.detail(format!("target: {}", args.target));

    let mut wasm_pack = Command::new("wasm-pack");
    wasm_pack
        .arg("build")
        .arg(&crate_path)
        .arg("--target")
        .arg(&args.target)
        .current_dir(project.root());
    if let Some(scope) = &args.scope {
        wasm_pack.arg("--scope").arg(scope);
    }
    if let Some(out_name) = &args.out_name {
        wasm_pack.arg("--out-name").arg(out_name);
    }
    if args.dev {
        wasm_pack.arg("--dev");
    }

    let status = wasm_pack.status().map_err(|e| CliError::Io {
        path: "wasm-pack".to_string(),
        reason: format!(
            "could not run wasm-pack — is it installed? (https://rustwasm.github.io/wasm-pack/): {e}"
        ),
    })?;
    if !status.success() {
        return Err(CliError::BuildFailed {
            code: status.code().unwrap_or(-1),
        });
    }

    if let Some(npm_name) = &args.npm_name {
        rewrite_package_name(&crate_path.join("pkg").join("package.json"), npm_name, ui)?;
    }

    ui.status(format!("done: {}/pkg", args.crate_path));
    Ok(())
}

/// Rewrite the `name` of a `wasm-pack`-generated `package.json`.
///
/// # Errors
/// [`CliError::Io`] if the file cannot be read, is not JSON, or cannot be written.
fn rewrite_package_name(path: &Path, name: &str, ui: &Ui) -> CliResult<()> {
    let io = |reason: String| CliError::Io {
        path: path.display().to_string(),
        reason,
    };
    let raw = std::fs::read_to_string(path)
        .map_err(|e| io(format!("could not read the generated package.json: {e}")))?;
    let mut manifest: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| io(format!("package.json is not JSON: {e}")))?;
    manifest["name"] = serde_json::Value::String(name.to_string());
    let mut serialized = serde_json::to_string_pretty(&manifest)
        .map_err(|e| io(format!("could not serialize package.json: {e}")))?;
    serialized.push('\n');
    std::fs::write(path, serialized)
        .map_err(|e| io(format!("could not write package.json: {e}")))?;
    ui.detail(format!("package name: {name}"));
    Ok(())
}
