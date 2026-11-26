//! A project's settings: `fun.toml`, what development runs on, and each file that overrides it —
//! `fun.<environment>.toml` for an environment, `fun.local.toml` for one developer's machine.

use std::fs;
use std::path::Path;

use fundamental_core::Error as _;
use fundamental_manifest::{KeyPath, Manifest, ManifestError, ManifestFinding};
use tempfile::TempDir;

type Outcome = Result<(), Box<dyn std::error::Error>>;

const BASE: &str = "[project]\nname = \"x\"\n\n[apps.www.web]\nport = 3000\nhost = \"127.0.0.1\"\n\n[local]\ndatabases = [\"www\", \"ops\"]\n";

fn project(files: &[(&str, &str)]) -> Result<TempDir, std::io::Error> {
    let directory = tempfile::tempdir()?;
    for (file, text) in files {
        let path = directory.path().join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
    }
    Ok(directory)
}

fn key(text: &str) -> KeyPath {
    text.parse()
        .unwrap_or_else(|error| panic!("{text}: {error}"))
}

/// The findings a refused load names, as `(code, message)`.
fn findings(result: Result<Manifest, ManifestError>) -> Vec<(String, String)> {
    match result {
        Err(ManifestError::Refused { findings, .. }) => findings
            .iter()
            .map(|finding| (finding.error_code().to_owned(), finding.to_string()))
            .collect(),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn development_reads_fun_toml() -> Outcome {
    let root = project(&[("fun.toml", BASE)])?;
    let manifest = Manifest::load(root.path())?;
    assert_eq!(
        manifest.get(&key("apps.www.web.port")),
        Some(&toml::Value::Integer(3000))
    );
    assert_eq!(manifest.files(), ["fun.toml"].map(std::path::PathBuf::from));
    Ok(())
}

#[test]
fn the_machines_file_overrides_development_key_by_key() -> Outcome {
    let root = project(&[
        ("fun.toml", BASE),
        (
            "fun.local.toml",
            "[apps.www.web]\nport = 4000\n\n[local]\ndatabases = [\"www\"]\n",
        ),
    ])?;
    let manifest = Manifest::load(root.path())?;
    assert_eq!(
        manifest.get(&key("apps.www.web.port")),
        Some(&toml::Value::Integer(4000)),
        "the value it names"
    );
    assert_eq!(
        manifest.get(&key("apps.www.web.host")),
        Some(&toml::Value::String("127.0.0.1".into())),
        "the rest of the table as fun.toml has it"
    );
    assert_eq!(
        manifest.get(&key("local.databases")),
        Some(&toml::Value::Array(vec![toml::Value::String("www".into())])),
        "a list replaces the list"
    );
    assert_eq!(
        manifest.files(),
        ["fun.toml", "fun.local.toml"].map(std::path::PathBuf::from)
    );
    Ok(())
}

#[test]
fn an_environment_is_fun_toml_under_its_own_file_and_never_the_machines() -> Outcome {
    let root = project(&[
        ("fun.toml", BASE),
        ("fun.production.toml", "[apps.www.web]\nport = 8080\n"),
        ("fun.local.toml", "[apps.www.web]\nhost = \"0.0.0.0\"\n"),
    ])?;
    let production = Manifest::load_for(root.path(), "production")?;
    assert_eq!(
        production.get(&key("apps.www.web.port")),
        Some(&toml::Value::Integer(8080))
    );
    assert_eq!(
        production.get(&key("apps.www.web.host")),
        Some(&toml::Value::String("127.0.0.1".into())),
        "fun.local.toml is one machine's, never an environment's"
    );
    assert_eq!(production.environment(), "production");
    let staging = Manifest::load_for(root.path(), "staging")?;
    assert_eq!(
        staging.get(&key("apps.www.web.port")),
        Some(&toml::Value::Integer(3000)),
        "an environment with no file of its own is fun.toml"
    );
    let development = Manifest::load_for(root.path(), "development")?;
    assert_eq!(
        development.get(&key("apps.www.web.host")),
        Some(&toml::Value::String("0.0.0.0".into())),
        "development is fun.toml and the machine's file, as `load` reads it"
    );
    Ok(())
}

#[test]
fn fun_development_toml_is_refused_because_development_is_fun_toml() -> Outcome {
    let root = project(&[
        ("fun.toml", BASE),
        ("fun.development.toml", "[apps.www.web]\nport = 1\n"),
    ])?;
    let found = findings(Manifest::load(root.path()));
    assert!(
        found.iter().any(|(code, message)| code == "MANIFEST_016"
            && message.contains("fun.development.toml")
            && message.contains("fun.toml")),
        "{found:?}"
    );
    Ok(())
}

#[test]
fn an_old_layout_is_refused_with_the_name_it_moves_to() -> Outcome {
    let root = project(&[
        ("fundamental.toml", BASE),
        ("fundamental/apps/www.toml", "[apps.www]\nbrand = \"x\"\n"),
        ("config/production.toml", "[apps.www.web]\nport = 8080\n"),
    ])?;
    let found = findings(Manifest::load(root.path()));
    let messages: Vec<&str> = found
        .iter()
        .filter(|(code, _)| code == "MANIFEST_017")
        .map(|(_, message)| message.as_str())
        .collect();
    for (old, new) in [
        ("fundamental.toml", "fun.toml"),
        ("fundamental/", "fun.toml"),
        ("config/production.toml", "fun.production.toml"),
    ] {
        assert!(
            messages
                .iter()
                .any(|message| message.contains(&format!("`{old}`")) && message.contains(new)),
            "{old} → {new} in {messages:?}"
        );
    }
    Ok(())
}

#[test]
fn a_folder_named_fundamental_that_holds_a_project_is_not_the_old_layout() -> Outcome {
    let root = project(&[
        ("fun.toml", BASE),
        (
            "fundamental/Cargo.toml",
            "[package]\nname = \"fundamental\"\n",
        ),
        (
            "fundamental/fun.toml",
            "[project]\nname = \"fundamental\"\n",
        ),
    ])?;
    let manifest = Manifest::load(root.path())?;
    assert_eq!(
        manifest.get(&key("project.name")),
        Some(&toml::Value::String("x".into()))
    );
    Ok(())
}

#[test]
fn an_override_may_not_turn_a_table_into_a_value() -> Outcome {
    let root = project(&[
        ("fun.toml", BASE),
        ("fun.local.toml", "[apps.www]\nweb = 4000\n"),
    ])?;
    let found = findings(Manifest::load(root.path()));
    assert!(
        found.iter().any(|(code, message)| code == "MANIFEST_018"
            && message.contains("fun.local.toml:2:1")
            && message.contains("apps.www.web")),
        "{found:?}"
    );
    Ok(())
}

#[test]
fn origins_name_file_and_line() -> Outcome {
    let root = project(&[
        ("fun.toml", BASE),
        ("fun.production.toml", "\n[apps.www.web]\nport = 8080\n"),
    ])?;
    let manifest = Manifest::load_for(root.path(), "production")?;
    let port: Vec<String> = manifest
        .origins(&key("apps.www.web.port"))
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(port, ["fun.production.toml:3:1"]);
    let host: Vec<String> = manifest
        .origins(&key("apps.www.web.host"))
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(host, ["fun.toml:6:1"]);
    Ok(())
}

#[test]
fn every_finding_is_named_in_one_load() -> Outcome {
    let root = project(&[
        ("fun.toml", "[project]\nname = \"x\"\n[apps\n"),
        ("fun.local.toml", "port = [\n"),
        ("fun.development.toml", ""),
    ])?;
    let found = findings(Manifest::load(root.path()));
    let files: Vec<bool> = ["fun.toml", "fun.local.toml", "fun.development.toml"]
        .iter()
        .map(|file| found.iter().any(|(_, message)| message.contains(file)))
        .collect();
    assert_eq!(files, [true, true, true], "{found:?}");
    Ok(())
}

#[test]
fn a_table_reads_as_its_type_or_names_where_it_is_wrong() -> Outcome {
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Web {
        port: u16,
        host: String,
    }
    let root = project(&[
        ("fun.toml", BASE),
        ("fun.local.toml", "[apps.www.web]\nport = \"one\"\n"),
    ])?;
    let manifest = Manifest::load(root.path())?;
    assert_eq!(manifest.table::<Web>(&key("apps.absent"))?, None);
    let wrong = manifest.table::<Web>(&key("apps.www.web"));
    assert!(
        matches!(&wrong, Err(ManifestError::Shape { at, .. }) if at.contains("fun.local.toml:2:1")),
        "{wrong:?}"
    );
    let production = Manifest::load_for(root.path(), "production")?;
    assert_eq!(
        production.table::<Web>(&key("apps.www.web"))?,
        Some(Web {
            port: 3000,
            host: "127.0.0.1".to_owned()
        })
    );
    Ok(())
}

#[test]
fn the_whole_manifest_reads_as_a_type() -> Outcome {
    #[derive(serde::Deserialize)]
    struct Whole {
        project: toml::Table,
        apps: toml::Table,
    }
    let root = project(&[("fun.toml", BASE)])?;
    let whole: Whole = Manifest::load(root.path())?.whole()?;
    assert_eq!(
        whole.project.get("name").and_then(toml::Value::as_str),
        Some("x")
    );
    assert!(whole.apps.contains_key("www"));
    Ok(())
}

#[test]
fn discover_walks_up_to_the_project() -> Outcome {
    let root = project(&[("fun.toml", BASE)])?;
    let deep = root.path().join("crates/a/src");
    fs::create_dir_all(&deep)?;
    assert_eq!(
        Manifest::discover(&deep)?.get(&key("project.name")),
        Some(&toml::Value::String("x".into()))
    );
    let nowhere = tempfile::tempdir()?;
    assert!(matches!(
        Manifest::discover(nowhere.path()),
        Err(ManifestError::NotFound { .. })
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_linked_root_reads_one_release() -> Outcome {
    let releases = tempfile::tempdir()?;
    for (release, port) in [("old", 1), ("new", 2)] {
        let directory = releases.path().join(release);
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("fun.toml"), BASE)?;
        fs::write(
            directory.join("fun.production.toml"),
            format!("[apps.www.web]\nport = {port}\n"),
        )?;
    }
    let home = tempfile::tempdir()?;
    std::os::unix::fs::symlink(
        releases.path().join("new/fun.toml"),
        home.path().join("fun.toml"),
    )?;
    // An environment's file beside the link belongs to no release and is never read.
    fs::write(
        home.path().join("fun.production.toml"),
        "[apps.www.web]\nport = 9\n",
    )?;
    let manifest = Manifest::load_for(home.path(), "production")?;
    assert_eq!(
        manifest.get(&key("apps.www.web.port")),
        Some(&toml::Value::Integer(2))
    );
    assert_eq!(manifest.root(), home.path());
    assert_eq!(
        manifest.source_dir(),
        releases.path().join("new").canonicalize()?
    );
    Ok(())
}

#[test]
fn a_finding_is_a_value_a_caller_can_match() {
    let finding = ManifestFinding::Renamed {
        old: "fundamental.toml".into(),
        new: "fun.toml".into(),
    };
    assert_eq!(finding.error_code(), "MANIFEST_017");
    assert!(Path::new(Manifest::FILE).ends_with("fun.toml"));
}
