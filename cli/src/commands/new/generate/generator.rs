//! The [`generate`] entry point that writes a scaffolded project to disk.

use std::path::Path;
use std::process::Command;

use super::ProjectPlan;
use super::app_templates::{app_cargo_toml, app_lib_rs, app_main_rs};
use super::native_templates::{
    ICON_FILES, android_build_gradle, android_hooks_plugin_kt, android_manifest, hooks_build_rs,
    hooks_cargo_toml, hooks_lib_rs, ios_hooks_plugin_swift, ios_package_swift, native_build_rs,
    native_capabilities_json, native_cargo_toml, native_desktop_hooks_rs, native_lib_rs,
    native_main_rs, native_platform_hooks_rs, native_platforms_readme, native_tauri_conf_json,
};
use super::templates::{
    clippy_toml, domain_actions_mod_rs, domain_cargo_toml, domain_lib_rs, domain_models_mod_rs,
    domain_sync_mod_rs, fun_toml, gitignore, nextest_toml, readme, tailwind_input_css,
    web_app_cargo_toml, web_build_rs, web_document_rs, web_lib_rs, web_live_boot_rs,
    web_page_index_rs, workspace_cargo_toml,
};

use super::islands_templates::{islands_cargo_toml, islands_lib_rs};
use crate::CliResult;
use crate::helpers::{write_binary_file, write_file};

/// Write the project described by `plan` to disk and initialize a git repo (skipped on failure).
///
/// # Errors
/// [`CliError::Io`](crate::CliError::Io) if any directory or file cannot be written.
pub(crate) fn generate(plan: &ProjectPlan) -> CliResult<()> {
    let root = plan.path.as_path();

    write_file(&root.join("fun.toml"), &fun_toml(plan))?;
    write_file(&root.join("Cargo.toml"), &workspace_cargo_toml(plan))?;
    write_file(&root.join("clippy.toml"), &clippy_toml())?;
    write_file(&root.join(".gitignore"), &gitignore())?;
    write_file(&root.join(".config").join("nextest.toml"), &nextest_toml())?;
    write_file(&root.join("README.md"), &readme(plan))?;

    let domain = root.join("domain");
    write_file(&domain.join("Cargo.toml"), &domain_cargo_toml(plan))?;
    write_file(&domain.join("src").join("lib.rs"), &domain_lib_rs(plan))?;
    write_file(
        &domain.join("src").join("models").join("mod.rs"),
        &domain_models_mod_rs(),
    )?;
    write_file(
        &domain.join("src").join("actions").join("mod.rs"),
        &domain_actions_mod_rs(),
    )?;
    // The `sync` module is feature-gated, so its `mod.rs` must exist for a sync build.
    write_file(
        &domain.join("src").join("sync").join("mod.rs"),
        &domain_sync_mod_rs(),
    )?;

    if plan.has_app() {
        generate_app_crate(plan, &root.join("apps").join(&plan.app_name))?;
    }

    init_git(root);

    Ok(())
}

/// Write one app crate into `app_dir`, as `fun new` and `fun add app` both do.
///
/// # Errors
/// [`CliError::Io`](crate::CliError::Io) if either file cannot be written.
pub(crate) fn generate_app_crate(plan: &ProjectPlan, app_dir: &Path) -> CliResult<()> {
    if plan.is_web_shaped() {
        generate_web_app_crate(plan, app_dir)?;
    } else if plan.has_app() {
        write_file(&app_dir.join("Cargo.toml"), &app_cargo_toml(plan))?;
        write_file(&app_dir.join("src").join("lib.rs"), &app_lib_rs(plan))?;
        write_file(&app_dir.join("src").join("main.rs"), &app_main_rs(plan))?;
    }
    Ok(())
}

/// Write the web app crate: server binary, wasm library, build script, and `pages/`.
///
/// # Errors
/// [`CliError::Io`](crate::CliError::Io) if any file cannot be written.
fn generate_web_app_crate(plan: &ProjectPlan, app_dir: &Path) -> CliResult<()> {
    write_file(&app_dir.join("Cargo.toml"), &web_app_cargo_toml(plan))?;
    write_file(&app_dir.join("build.rs"), &web_build_rs(plan))?;
    // A native-only app crate is a library hosted by the sibling `native/` crate.
    if plan.web {
        write_file(&app_dir.join("src").join("main.rs"), &app_main_rs(plan))?;
    }
    write_file(&app_dir.join("src").join("lib.rs"), &web_lib_rs(plan))?;
    if plan.sync {
        write_file(
            &app_dir.join("src").join("live_boot.rs"),
            &web_live_boot_rs(plan),
        )?;
    }
    write_file(
        &app_dir.join("pages").join("index.rs"),
        &web_page_index_rs(plan.has_islands_crate()),
    )?;
    if plan.has_islands_crate() {
        let islands = app_dir.join("islands");
        write_file(
            &islands.join("Cargo.toml"),
            &islands_cargo_toml(&plan.app_crate_name, &plan.domain),
        )?;
        write_file(
            &islands.join("src").join("lib.rs"),
            &islands_lib_rs(&plan.app_name, &plan.app_crate_name),
        )?;
    }
    if let Some(document) = web_document_rs(plan) {
        write_file(&app_dir.join("pages").join("document.rs"), &document)?;
    }
    if plan.tailwind {
        write_file(
            &app_dir.join("styles").join("input.css"),
            &tailwind_input_css(),
        )?;
    }
    if plan.native {
        generate_native_crate(plan, &app_dir.join("native"))?;
    }
    Ok(())
}

/// Write the native crate tree under `native_dir`: host crate, hooks plugin and `platforms/`.
///
/// # Errors
/// [`CliError::Io`](crate::CliError::Io) if any file cannot be written.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(crate) fn generate_native_crate(plan: &ProjectPlan, native_dir: &Path) -> CliResult<()> {
    write_file(&native_dir.join("Cargo.toml"), &native_cargo_toml(plan))?;
    write_file(&native_dir.join("build.rs"), &native_build_rs())?;
    write_file(
        &native_dir.join("tauri.conf.json"),
        &native_tauri_conf_json(plan),
    )?;
    write_file(
        &native_dir.join("capabilities").join("default.json"),
        &native_capabilities_json(),
    )?;
    write_file(&native_dir.join("src").join("lib.rs"), &native_lib_rs(plan))?;
    write_file(
        &native_dir.join("src").join("main.rs"),
        &native_main_rs(plan),
    )?;
    write_file(
        &native_dir.join("src").join("platform_hooks.rs"),
        &native_platform_hooks_rs(),
    )?;

    let icons = native_dir.join("icons");
    for (name, bytes) in ICON_FILES {
        write_binary_file(&icons.join(name), bytes)?;
    }

    let platforms = native_dir.join("platforms");
    write_file(&platforms.join("README.md"), &native_platforms_readme(plan))?;
    for os in ["macos", "windows", "linux"] {
        write_file(
            &platforms.join(os).join("hooks.rs"),
            &native_desktop_hooks_rs(os),
        )?;
    }
    write_file(
        &platforms.join("ios").join("Package.swift"),
        &ios_package_swift(plan),
    )?;
    write_file(
        &platforms
            .join("ios")
            .join("Sources")
            .join("HooksPlugin.swift"),
        &ios_hooks_plugin_swift(),
    )?;
    write_file(
        &platforms.join("android").join("build.gradle.kts"),
        &android_build_gradle(plan),
    )?;
    write_file(
        &platforms
            .join("android")
            .join("src")
            .join("main")
            .join("AndroidManifest.xml"),
        &android_manifest(),
    )?;
    write_file(
        &platforms
            .join("android")
            .join("src")
            .join("main")
            .join("java")
            .join("HooksPlugin.kt"),
        &android_hooks_plugin_kt(plan),
    )?;

    let hooks = native_dir.join("hooks");
    write_file(&hooks.join("Cargo.toml"), &hooks_cargo_toml(plan))?;
    write_file(&hooks.join("build.rs"), &hooks_build_rs())?;
    write_file(&hooks.join("src").join("lib.rs"), &hooks_lib_rs(plan))?;

    Ok(())
}

/// A missing or failing `git` is logged and skipped: the project is usable without it.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn init_git(root: &Path) {
    match Command::new("git").arg("init").arg(root).output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::warn!(status = ?out.status.code(), %stderr, "git init failed; skipping");
        }
        Err(e) => {
            tracing::warn!(error = %e, "git not available; skipping repository init");
        }
    }
}
