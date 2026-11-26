//! A crate's build holds its messages to the policy: a missing translation fails it where the
//! policy makes that an error, and falls back to the source where it warns.

use std::path::Path;

use fundamental_messages::Catalogs;
use fundamental_messages_build::{Environment, Gap, Level, Policy, build};

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
    std::fs::write(path, text).expect("the file is written");
}

fn crate_missing_dutch() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a folder");
    write(
        root.path(),
        "locales/en/panel.mf2",
        "@locale en\n---\n[panel]\nsave = Save\ncancel = Cancel\n",
    );
    write(
        root.path(),
        "locales/nl/panel.mf2",
        "@locale nl\n---\n[panel]\nsave = Opslaan\n",
    );
    root
}

#[test]
fn a_missing_translation_fails_the_build_where_the_policy_makes_it_an_error() {
    let root = crate_missing_dutch();
    let out = tempfile::tempdir().expect("a folder");
    let policy = Policy {
        missing: Level::Error,
        ..Policy::default_for(Environment::Local)
    };
    assert!(build(root.path(), out.path(), &policy).is_err());
}

#[test]
fn a_missing_translation_falls_back_where_the_policy_warns() {
    let root = crate_missing_dutch();
    let out = tempfile::tempdir().expect("a folder");
    build(
        root.path(),
        out.path(),
        &Policy::default_for(Environment::Local),
    )
    .expect("the build goes on");
    assert!(out.path().join("messages.rs").is_file());
}

/// The catalogs a build wrote to `out`, as the crate's `CATALOGS` holds them, with `chains`.
fn built(
    out: &Path,
    chains: &'static [(&'static str, &'static [&'static str])],
) -> &'static Catalogs {
    let code = std::fs::read_to_string(out.join("messages.rs")).expect("the module is written");
    let manifest = code
        .split("Catalogs::new(\"en\", 0x")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .expect("the module names the catalogs' manifest");
    let files: Vec<(&'static str, &'static [u8], &'static [u8])> = ["en", "nl"]
        .into_iter()
        .map(|tag| {
            let read = |end: &str| -> &'static [u8] {
                Box::leak(
                    std::fs::read(out.join(format!("{tag}.{end}")))
                        .expect("the catalog is written")
                        .into_boxed_slice(),
                )
            };
            (tag, read("catalog"), read("server"))
        })
        .collect();
    Box::leak(Box::new(
        Catalogs::new("en", manifest, Box::leak(files.into_boxed_slice())).falling_back(chains),
    ))
}

#[test]
fn a_gap_says_what_the_policy_shows() {
    let root = crate_missing_dutch();
    for (gap, said) in [
        (Gap::Borrowed, "Cancel"),
        (Gap::Id, "panel.cancel"),
        (Gap::Nothing, ""),
    ] {
        let out = tempfile::tempdir().expect("a folder");
        let policy = Policy {
            gap,
            ..Policy::default_for(Environment::Local)
        };
        build(root.path(), out.path(), &policy).expect("the build goes on");
        assert_eq!(
            built(out.path(), &[])
                .say("panel.cancel", "nl", &[])
                .as_deref(),
            Some(said),
            "{gap:?}"
        );
    }
}

#[test]
fn a_reader_s_language_the_crate_lacks_is_said_in_the_first_its_chain_names() {
    let root = crate_missing_dutch();
    let out = tempfile::tempdir().expect("a folder");
    let mut policy = Policy::default_for(Environment::Local);
    policy
        .fallback
        .insert("fy".to_owned(), vec!["nl".to_owned()]);
    build(root.path(), out.path(), &policy).expect("the build goes on");
    let code = std::fs::read_to_string(out.path().join("messages.rs")).expect("the module");
    assert!(
        code.contains(r#".falling_back(&[("fy", &["nl"])])"#),
        "{code}"
    );
    let catalogs = built(out.path(), &[("fy", &["nl"])]);
    assert_eq!(
        catalogs.say("panel.save", "fy", &[]).as_deref(),
        Some("Opslaan")
    );
    assert_eq!(
        catalogs.say("panel.save", "fy-NL", &[]).as_deref(),
        Some("Opslaan")
    );
    assert_eq!(
        catalogs.say("panel.save", "de", &[]).as_deref(),
        Some("Save")
    );
}
