//! `fun bench`: its layers on the command line, and a workspace with no bench of a layer's name
//! told what to add.

use std::fs;
use std::process::{Command, Output};

use tempfile::TempDir;

fn fun(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fun"))
        .arg("--quiet")
        .args(args)
        .current_dir(dir)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .expect("fun runs")
}

#[test]
fn the_help_names_every_layer() {
    let dir = TempDir::new().expect("a directory");
    let help = fun(dir.path(), &["bench", "--help"]);
    assert!(help.status.success(), "{help:?}");
    let text = String::from_utf8_lossy(&help.stdout);
    for layer in ["instructions", "wall", "load", "soak", "fault", "--save"] {
        assert!(text.contains(layer), "{layer} is not in:\n{text}");
    }
}

#[test]
fn an_unknown_layer_is_refused() {
    let dir = TempDir::new().expect("a directory");
    assert!(!fun(dir.path(), &["bench", "everything"]).status.success());
}

#[test]
fn a_workspace_without_the_layers_bench_is_told_what_to_add() {
    let dir = TempDir::new().expect("a directory");
    fs::write(dir.path().join("fun.toml"), "[project]\nname = \"acme\"\n").expect("fun.toml");
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"acme\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("Cargo.toml");
    fs::create_dir_all(dir.path().join("src")).expect("src");
    fs::write(dir.path().join("src/lib.rs"), "").expect("lib.rs");
    let run = fun(dir.path(), &["bench", "wall"]);
    assert!(!run.status.success(), "{run:?}");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(said.contains("benches/wall.rs"), "{said}");
}
