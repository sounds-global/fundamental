//! What a missing, stale or unused message is, is a policy: a level per environment, set in the
//! project's `fun.toml` under `[messages]`, an error in CI and a warning on a developer's
//! machine unless it says otherwise.

use fundamental_messages_build::{
    Environment, Finding, Gap, Kind, Language, Level, Policy, Report,
};

#[test]
fn without_a_policy_a_gap_warns_locally_and_fails_in_ci() {
    let root = tempfile::tempdir().expect("a folder");
    let local = Policy::read(root.path(), Environment::Local).expect("the policy is read");
    assert_eq!(
        (local.missing, local.stale, local.unused),
        (Level::Warn, Level::Error, Level::Warn)
    );
    let ci = Policy::read(root.path(), Environment::Ci).expect("the policy is read");
    assert_eq!(
        (ci.missing, ci.stale, ci.unused),
        (Level::Error, Level::Error, Level::Error)
    );
}

#[test]
fn the_project_sets_each_level_per_environment() {
    let root = tempfile::tempdir().expect("a folder");
    std::fs::write(
        root.path().join("fun.toml"),
        "[project]\nname = \"x\"\n\n[messages.local]\nmissing = \"error\"\n\n[messages.ci]\nunused = \"allow\"\n",
    )
    .expect("the manifest is written");
    let crate_root = root.path().join("crates/one");
    std::fs::create_dir_all(&crate_root).expect("the crate's folder");

    let local = Policy::read(&crate_root, Environment::Local).expect("the policy is read");
    assert_eq!(local.missing, Level::Error);
    assert_eq!(local.unused, Level::Warn);
    let ci = Policy::read(&crate_root, Environment::Ci).expect("the policy is read");
    assert_eq!(ci.unused, Level::Allow);
    assert_eq!(ci.missing, Level::Error);
}

/// A policy this machine's `fun.local.toml` sets is read over `fun.toml`'s, and a build reruns
/// when either file changes.
#[test]
fn the_machines_policy_is_read_over_the_projects() {
    let root = tempfile::tempdir().expect("a folder");
    std::fs::write(
        root.path().join("fun.toml"),
        "[project]\nname = \"x\"\n\n[messages.local]\nmissing = \"warn\"\n",
    )
    .expect("the settings are written");
    std::fs::write(
        root.path().join("fun.local.toml"),
        "[messages.local]\nmissing = \"error\"\nunused = \"error\"\n",
    )
    .expect("the machine's policy is written");
    let crate_root = root.path().join("domain/one");
    std::fs::create_dir_all(&crate_root).expect("the crate's folder");

    let local = Policy::read(&crate_root, Environment::Local).expect("the policy is read");
    assert_eq!((local.missing, local.unused), (Level::Error, Level::Error));
    let watched = Policy::manifests(&crate_root);
    for file in ["fun.toml", "fun.local.toml"] {
        assert!(
            watched.contains(&root.path().join(file)),
            "a build reruns when {file} changes: {watched:?}"
        );
    }
}

#[test]
fn a_level_that_is_not_one_is_refused() {
    let root = tempfile::tempdir().expect("a folder");
    std::fs::write(
        root.path().join("fun.toml"),
        "[messages.ci]\nmissing = \"loud\"\n",
    )
    .expect("the manifest is written");
    let refused = Policy::read(root.path(), Environment::Ci).expect_err("refused");
    assert!(refused.contains("loud"), "{refused}");
}

#[test]
fn ci_is_where_the_ci_variable_says_so() {
    assert_eq!(Environment::from_ci(Some("true")), Environment::Ci);
    assert_eq!(Environment::from_ci(Some("1")), Environment::Ci);
    assert_eq!(Environment::from_ci(Some("false")), Environment::Local);
    assert_eq!(Environment::from_ci(Some("")), Environment::Local);
    assert_eq!(Environment::from_ci(None), Environment::Local);
}

/// An error raised without its message is judged as a missing one: no language says it. A
/// wording said with what its message is not given is judged as a stale one: it is out of date
/// with the message it words.
#[test]
fn each_gap_is_judged_at_its_level_and_an_allowed_one_is_not_reported() {
    let report = Report {
        source: "en".to_owned(),
        languages: vec![Language {
            tag: "nl".to_owned(),
            missing: vec!["panel.save".to_owned()],
            stale: vec!["panel.cancel".to_owned()],
        }],
        unused: vec!["legacy.words".to_owned()],
        undeclared: vec!["error.shell_002".to_owned()],
        mismatched: vec!["label.rows".to_owned()],
    };
    let policy = Policy {
        missing: Level::Error,
        stale: Level::Warn,
        unused: Level::Allow,
        ..Policy::default_for(Environment::Local)
    };
    assert_eq!(
        report.findings(&policy),
        [
            Finding {
                level: Level::Error,
                kind: Kind::Missing,
                language: Some("nl".to_owned()),
                id: "panel.save".to_owned(),
            },
            Finding {
                level: Level::Warn,
                kind: Kind::Stale,
                language: Some("nl".to_owned()),
                id: "panel.cancel".to_owned(),
            },
            Finding {
                level: Level::Error,
                kind: Kind::Undeclared,
                language: Some("en".to_owned()),
                id: "error.shell_002".to_owned(),
            },
            Finding {
                level: Level::Warn,
                kind: Kind::Mismatched,
                language: Some("en".to_owned()),
                id: "label.rows".to_owned(),
            },
        ]
    );
}

#[test]
fn a_gap_borrows_from_its_chain_unless_the_project_shows_its_id_or_nothing() {
    let root = tempfile::tempdir().expect("a folder");
    assert_eq!(
        Policy::read(root.path(), Environment::Ci)
            .expect("the policy is read")
            .gap,
        Gap::Borrowed
    );
    std::fs::write(
        root.path().join("fun.toml"),
        "[messages.local]\ngap = \"id\"\n\n[messages.ci]\ngap = \"nothing\"\n",
    )
    .expect("the manifest is written");
    let local = Policy::read(root.path(), Environment::Local).expect("the policy is read");
    assert_eq!(local.gap, Gap::Id);
    let ci = Policy::read(root.path(), Environment::Ci).expect("the policy is read");
    assert_eq!(ci.gap, Gap::Nothing);
}

#[test]
fn a_gap_that_is_not_one_is_refused() {
    let root = tempfile::tempdir().expect("a folder");
    std::fs::write(
        root.path().join("fun.toml"),
        "[messages.ci]\ngap = \"blank\"\n",
    )
    .expect("the manifest is written");
    let refused = Policy::read(root.path(), Environment::Ci).expect_err("refused");
    assert!(refused.contains("blank"), "{refused}");
}

#[test]
fn the_project_names_each_language_s_fallback_chain() {
    let root = tempfile::tempdir().expect("a folder");
    std::fs::write(
        root.path().join("fun.toml"),
        "[messages.fallback]\nfy = [\"nl\"]\n\"nl-BE\" = [\"nl\"]\n",
    )
    .expect("the manifest is written");
    let policy = Policy::read(root.path(), Environment::Local).expect("the policy is read");
    assert_eq!(
        policy.fallback.get("fy").map(Vec::as_slice),
        Some(["nl".to_owned()].as_slice())
    );
    assert_eq!(policy.fallback.len(), 2);
}

#[test]
fn a_chain_that_is_not_a_list_of_languages_is_refused() {
    let root = tempfile::tempdir().expect("a folder");
    std::fs::write(
        root.path().join("fun.toml"),
        "[messages.fallback]\nfy = \"nl\"\n",
    )
    .expect("the manifest is written");
    let refused = Policy::read(root.path(), Environment::Local).expect_err("refused");
    assert!(refused.contains("fy"), "{refused}");
}
