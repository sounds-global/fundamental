//! The editor writes `fun.toml`, keeps its comments, and never writes over a change it did not
//! see or into a file people own.

use std::fs;

use fundamental_manifest::{Manifest, ManifestEditor, ManifestError};
use tempfile::TempDir;
use toml_edit::value;

type Outcome = Result<(), Box<dyn std::error::Error>>;

fn project() -> Result<TempDir, std::io::Error> {
    let directory = tempfile::tempdir()?;
    fs::write(
        directory.path().join("fun.toml"),
        "[project]\nname = \"x\"\n\n# The site.\n[apps.www]\nweb = { port = 1 }\n",
    )?;
    fs::write(
        directory.path().join("fun.local.toml"),
        "[apps.www.web]\nport = 2\n",
    )?;
    Ok(directory)
}

#[test]
fn the_editor_writes_fun_toml_keeping_its_comments() -> Outcome {
    let project = project()?;
    let mut editor = ManifestEditor::open(project.path())?;
    editor
        .table_mut(&"commands.gate".parse()?)?
        .insert("infra", value("scripts/infra.sh"));
    editor
        .table_mut(&"apps.www".parse()?)?
        .insert("admin", value("y"));
    assert!(editor.save()?, "a change is written");
    assert!(!editor.save()?, "a second save has nothing to write");
    let written = fs::read_to_string(project.path().join("fun.toml"))?;
    assert!(written.contains("# The site.\n[apps.www]"), "{written}");
    assert!(written.contains("[commands.gate]"), "{written}");
    assert_eq!(
        fs::read_to_string(project.path().join("fun.local.toml"))?,
        "[apps.www.web]\nport = 2\n",
        "the machine's file is a person's, never a command's"
    );
    let manifest = Manifest::load(project.path())?;
    assert_eq!(
        manifest.get(&"apps.www.admin".parse()?),
        Some(&toml::Value::String("y".into()))
    );
    Ok(())
}

#[test]
fn a_file_changed_since_it_was_read_is_not_overwritten() -> Outcome {
    let project = project()?;
    let mut editor = ManifestEditor::open(project.path())?;
    editor
        .table_mut(&"apps.www".parse()?)?
        .insert("admin", value("y"));
    let by_hand = "[project]\nname = \"x\"\n\n[apps.www]\nweb = { port = 7 }\n";
    fs::write(project.path().join("fun.toml"), by_hand)?;
    let refused = editor.save();
    assert!(
        matches!(&refused, Err(ManifestError::ChangedSinceRead { file }) if file == "fun.toml"),
        "{refused:?}"
    );
    assert_eq!(
        fs::read_to_string(project.path().join("fun.toml"))?,
        by_hand
    );
    Ok(())
}

#[test]
fn a_value_cannot_be_opened_as_a_table() -> Outcome {
    let project = project()?;
    let mut editor = ManifestEditor::open(project.path())?;
    let refused = editor.table_mut(&"project.name".parse()?).map(|_| ());
    assert!(
        matches!(refused, Err(ManifestError::BadKey { .. })),
        "{refused:?}"
    );
    Ok(())
}
