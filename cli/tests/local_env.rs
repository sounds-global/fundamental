//! `fun local env`, run as a project's scripts run it: from the checkout, with the machine's
//! state in a directory of the test's own.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn project() -> TempDir {
    let dir = TempDir::new().expect("a temporary directory");
    fs::write(
        dir.path().join("fun.toml"),
        "[project]\nname = \"acme\"\n\n[local]\napexes = [\"acme.test\", \"acme-tenants.test\"]\n",
    )
    .expect("a manifest");
    dir
}

fn fun(checkout: &Path, state: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fun"))
        .args(["--quiet", "local"])
        .args(args)
        .current_dir(checkout)
        .env("XDG_STATE_HOME", state)
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("FUN_LOCAL_LABEL")
        .output()
        .expect("fun runs")
}

#[test]
fn names_are_printed_before_anything_is_claimed_and_claim_nothing() {
    let project = project();
    let state = TempDir::new().expect("a state directory");

    let names = fun(
        project.path(),
        state.path(),
        &["env", "--names", "--label", "ci"],
    );
    assert!(names.status.success(), "{names:?}");
    assert_eq!(
        String::from_utf8_lossy(&names.stdout),
        "FUN_LOCAL_PROJECT='acme'\n\
         FUN_LOCAL_ROLE='lane'\n\
         FUN_LOCAL_LABEL='ci'\n\
         FUN_LOCAL_SUFFIX_ACME_TENANTS='ci.acme-tenants-lane.test'\n\
         FUN_LOCAL_SUFFIX_ACME='ci.acme-lane.test'\n"
    );
    assert!(
        fs::read_dir(state.path())
            .expect("the state directory")
            .next()
            .is_none(),
        "printing names wrote machine state"
    );

    let facts = fun(project.path(), state.path(), &["env", "--label", "ci"]);
    assert!(!facts.status.success());
    assert!(
        String::from_utf8_lossy(&facts.stderr).contains("LOCAL_021"),
        "{facts:?}"
    );
}

#[test]
fn names_refuse_a_label_that_is_no_dns_label() {
    let project = project();
    let state = TempDir::new().expect("a state directory");
    let names = fun(
        project.path(),
        state.path(),
        &["env", "--names", "--label", "Not_A_Label"],
    );
    assert!(!names.status.success());
    assert!(
        String::from_utf8_lossy(&names.stderr).contains("LOCAL_019"),
        "{names:?}"
    );
}
