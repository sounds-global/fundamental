//! A project using the whole page pipeline builds for the server and the browser with no
//! `#[cfg]` in its pages, and a loader's word is served in the reader's language.

#![cfg(not(target_arch = "wasm32"))]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn framework() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn rust_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(rust_files(&path)?);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(files)
}

/// The fixture `name` as a standalone project at `project`, resolving from the framework's
/// lockfile so the build needs no network.
fn stage(project: &Path, name: &str, manifest: impl FnOnce(&str) -> String) -> io::Result<()> {
    if project.exists() {
        fs::remove_dir_all(project)?;
    }
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
        project,
    )?;
    let root = fs::canonicalize(framework())?;
    fs::write(
        project.join("Cargo.toml"),
        manifest(&root.display().to_string()),
    )?;
    fs::copy(framework().join("Cargo.lock"), project.join("Cargo.lock"))?;
    Ok(())
}

fn loader_page_manifest(root: &str) -> String {
    format!(
        r#"[package]
name = "loader_page_fixture"
version = "0.0.0"
edition = "2024"
publish = false

[features]
default = ["ssr"]
ssr = ["fundamental/web", "fundamental/authz-server", "leptos/ssr"]
hydrate = ["fundamental/web-hydrate", "leptos/hydrate", "dep:wasm-bindgen"]

[dependencies]
fundamental = {{ path = "{root}" }}
leptos = {{ version = "0.8", default-features = false, features = ["islands"] }}
wasm-bindgen = {{ version = "0.2", optional = true }}

[build-dependencies]
fundamental_build = {{ path = "{root}/crates/build" }}

[workspace]
"#
    )
}

/// The manifest of the `loader_language` fixture: the server build only.
fn loader_language_manifest(root: &str) -> String {
    format!(
        r#"[package]
name = "loader_language_fixture"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
fundamental = {{ path = "{root}", features = ["web"] }}
leptos = {{ version = "0.8", default-features = false, features = ["islands", "ssr"] }}
tokio = {{ version = "1", features = ["rt"] }}

[build-dependencies]
fundamental_build = {{ path = "{root}/crates/build" }}

[dev-dependencies]
tokio = {{ version = "1", features = ["rt-multi-thread", "macros"] }}
reqwest = {{ version = "0.12", default-features = false }}

[workspace]
"#
    )
}

/// Runs cargo in `project` with a build directory of its own under this run's `target/`, kept
/// between runs.
fn cargo(project: &Path, args: &[&str]) -> Output {
    let name = project
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| framework().join("target"), PathBuf::from);
    Command::new(env!("CARGO"))
        .args(args)
        .current_dir(project)
        .env("CARGO_TARGET_DIR", base.join("fixture-builds").join(name))
        .output()
        .unwrap_or_else(|error| panic!("spawn cargo {args:?}: {error}"))
}

fn assert_built(output: &Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn loader_page_builds_for_server_and_wasm() {
    let project = Path::new(env!("CARGO_TARGET_TMPDIR")).join("loader_page");
    stage(&project, "loader_page", loader_page_manifest).expect("the fixture is staged");

    for file in rust_files(&project.join("pages")).expect("the pages are listed") {
        let source = fs::read_to_string(&file).expect("a page file reads");
        assert!(
            !source.contains("#[cfg") && !source.contains("cfg!("),
            "{} names a target; a page never does",
            file.display()
        );
    }

    let server = cargo(
        &project,
        &["build", "--offline", "--lib", "--features", "ssr"],
    );
    assert_built(&server, "the server build");

    let browser = cargo(
        &project,
        &[
            "build",
            "--offline",
            "--lib",
            "--target",
            "wasm32-unknown-unknown",
            "--no-default-features",
            "--features",
            "hydrate",
        ],
    );
    assert_built(
        &browser,
        "the browser build (`rustup target add wasm32-unknown-unknown`)",
    );
}

#[test]
fn a_loader_builds_its_words_in_the_readers_language() {
    let project = Path::new(env!("CARGO_TARGET_TMPDIR")).join("loader_language");
    stage(&project, "loader_language", loader_language_manifest).expect("the fixture is staged");

    let served = cargo(&project, &["test", "--offline", "--test", "language"]);
    assert!(
        served.status.success(),
        "the fixture's pages did not answer in the reader's language:\n{}\n{}",
        String::from_utf8_lossy(&served.stdout),
        String::from_utf8_lossy(&served.stderr)
    );
}

fn write_tree(project: &Path, files: &[(&str, String)]) -> io::Result<()> {
    if project.exists() {
        fs::remove_dir_all(project)?;
    }
    for (path, contents) in files {
        let path = project.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
    }
    Ok(())
}

/// A workspace of `widgets`, which holds an `#[island]`, and the islands crate `app_islands`.
fn islands_crate_fixture(root: &str) -> Vec<(&'static str, String)> {
    vec![
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"widgets\", \"islands\"]\nresolver = \"3\"\n".to_owned(),
        ),
        (
            "widgets/Cargo.toml",
            "[package]\nname = \"widgets\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
             [dependencies]\nleptos = { version = \"0.8\", default-features = false, features = [\"islands\"] }\n"
                .to_owned(),
        ),
        (
            "widgets/src/lib.rs",
            "use leptos::prelude::*;\n\n\
             #[island]\n\
             pub fn IslandsCrateCounter() -> impl IntoView {\n\
             \x20   let (count, set) = signal(0);\n\
             \x20   view! { <button on:click=move |_| set.update(|n| *n += 1)>{move || count.get()}</button> }\n\
             }\n"
                .to_owned(),
        ),
        (
            "islands/Cargo.toml",
            format!(
                "[package]\nname = \"app_islands\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
                 [dependencies]\nwidgets = {{ path = \"../widgets\" }}\n\
                 fundamental = {{ path = \"{root}\", optional = true }}\n\
                 leptos = {{ version = \"0.8\", default-features = false, features = [\"islands\"], optional = true }}\n\
                 wasm-bindgen = {{ version = \"0.2\", optional = true }}\n\n\
                 [features]\n\
                 hydrate = [\"fundamental/web-hydrate\", \"dep:leptos\", \"leptos/hydrate\", \"dep:wasm-bindgen\"]\n"
            ),
        ),
        (
            "islands/src/lib.rs",
            "use widgets as _;\n\n\
             #[cfg(feature = \"hydrate\")]\n\
             #[wasm_bindgen::prelude::wasm_bindgen]\n\
             pub fn hydrate() {\n\
             \x20   fundamental::web::hydrate();\n\
             }\n"
                .to_owned(),
        ),
    ]
}

/// An islands crate's bundle exports the hydrate function of an island in a crate it depends on.
#[test]
fn an_islands_crate_bundles_the_islands_of_the_crates_it_uses() {
    let project = Path::new(env!("CARGO_TARGET_TMPDIR")).join("islands_crate");
    let root = fs::canonicalize(framework()).expect("the framework checkout");
    let root = root.display();
    write_tree(&project, &islands_crate_fixture(&root.to_string()))
        .expect("the fixture is written");
    fs::copy(framework().join("Cargo.lock"), project.join("Cargo.lock"))
        .expect("the framework's lockfile");

    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| framework().join("target"), PathBuf::from);
    let dist = project.join("dist");
    let bindgen = fundamental_build::locked_bindgen_cli(&project.join("Cargo.lock"))
        .expect("the locked wasm-bindgen CLI (scripts/wasm-test.sh installs it)");
    let bundle = fundamental_build::WasmBuild::new(
        project.join("islands/Cargo.toml"),
        base.join("fixture-builds").join("islands_crate"),
        &dist,
        "app_islands",
    )
    .out_name("app")
    .feature("hydrate")
    .bindgen(bindgen)
    .build()
    .unwrap_or_else(|error| panic!("the islands bundle builds: {error:?}"));

    let js = fs::read_to_string(&bundle.js).expect("the bundle's script");
    assert!(
        js.contains("IslandsCrateCounter_"),
        "the bundle exports no hydrate function for the dependency's island:\n{js}"
    );
    assert!(js.contains("export function hydrate"), "{js}");
}
