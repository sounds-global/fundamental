//! A crate's messages are checked per language: what a translation is missing, what it still
//! says that the source no longer has, and what the source declares that no code says.

use std::path::Path;

use fundamental_messages_build::check;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
    std::fs::write(path, text).expect("the file is written");
}

const SOURCE: &str = "@locale en
---
[panel]
save = Save
version = .input {$number :integer} {{Version {$number}}}
greeting = Hello

[kind]
milestone = Milestone

[error]
shell_001 = The settings are not valid

[legacy]
words = Words
";

#[test]
fn a_translation_is_missing_what_the_source_has_and_stale_in_what_it_no_longer_has() {
    let root = tempfile::tempdir().expect("a folder");
    write(root.path(), "locales/en/panel.mf2", SOURCE);
    write(
        root.path(),
        "locales/nl/panel.mf2",
        "@locale nl
---
[panel]
save = Opslaan
version = .input {$number :integer} {{Versie {$number}}}
cancel = Annuleren

[kind]
milestone = Mijlpaal
",
    );
    write(root.path(), "src/lib.rs", "");

    let report = check(root.path()).expect("the messages are read");

    let dutch = report
        .languages
        .iter()
        .find(|language| language.tag == "nl")
        .expect("Dutch is checked");
    assert_eq!(
        dutch.missing,
        ["error.shell_001", "legacy.words", "panel.greeting"]
    );
    assert_eq!(dutch.stale, ["panel.cancel"]);
    assert!(
        report.languages.iter().all(|language| language.tag != "en"),
        "the source language is what the others are checked against"
    );
}

/// A crate's code is every target's: its library, binaries, tests, examples and benches.
#[test]
fn a_message_no_code_says_is_unused() {
    let root = tempfile::tempdir().expect("a folder");
    write(root.path(), "locales/en/panel.mf2", SOURCE);
    write(
        root.path(),
        "locales/en/tested.mf2",
        "@locale en\n---\n[tested]\nonly = Said in a unit test\n",
    );
    write(
        root.path(),
        "src/lib.rs",
        r#"
fundamental_messages::messages!();
mod view;

/// What an article reports.
#[derive(Variant, Messages)]
#[messages(m::kind)]
pub enum Kind { Milestone }

#[derive(Debug, Error)]
pub enum ShellError {
    #[error(code = "SHELL_001", message = "The settings are not valid", status = 500)]
    Invalid,
}

// m::legacy::words() is only written in a comment.
const SAID: &str = "m::legacy::words";

#[cfg(test)]
mod tests {
    fn tested() -> String { crate::m::tested::only().to_string() }
}
"#,
    );
    write(
        root.path(),
        "src/view/mod.rs",
        "pub fn view() -> impl IntoView { view! { <button>{crate::m::panel::save()}</button> } }\n\
         pub fn line() -> String { m::panel::version(2).to_string() }\n",
    );
    write(
        root.path(),
        "tests/greets.rs",
        "#[test] fn greets() { assert_eq!(m::panel::greeting().format_in(\"en\"), \"Hello\"); }\n",
    );

    let report = check(root.path()).expect("the messages are read");

    assert_eq!(report.unused, ["legacy.words"]);
}

/// An app's pages and its seeds are its library, as its build includes them: a message a page
/// says is said, and an error a page raises is one of the app's.
#[test]
fn an_app_s_pages_and_seeds_say_its_messages() {
    let root = tempfile::tempdir().expect("a folder");
    write(
        root.path(),
        "locales/en/sign_in.mf2",
        "@locale en\n---\n[sign_in]\ntitle = Sign in\n\n[seed]\nnote = Seeded\n",
    );
    write(
        root.path(),
        "src/lib.rs",
        "fundamental_messages::messages!();\n",
    );
    write(
        root.path(),
        "pages/(team)/sign-in/index.rs",
        r#"
pub fn page() -> impl IntoView { view! { <h1>{crate::m::sign_in::title()}</h1> } }

#[derive(Debug, Error)]
pub enum PageError {
    #[error(code = "APP_001", message = "The page cannot be read", status = 500)]
    Unreadable,
}
"#,
    );
    write(
        root.path(),
        "seeds/mod.rs",
        "pub fn note() -> String { crate::m::seed::note().to_string() }\n",
    );

    let report = check(root.path()).expect("the messages are read");

    assert!(report.unused.is_empty(), "{report}");
    assert_eq!(report.undeclared, ["error.app_001"]);
}

/// Only the crate's own errors are its messages: an error a test makes up is no reader's.
#[test]
fn an_error_the_code_raises_without_a_message_is_undeclared() {
    let root = tempfile::tempdir().expect("a folder");
    write(
        root.path(),
        "locales/en/errors.mf2",
        "@locale en\n---\n[error]\nshell_001 = The settings are not valid\n",
    );
    write(
        root.path(),
        "src/lib.rs",
        r#"
#[derive(Debug, Error)]
pub enum ShellError {
    #[error(code = "SHELL_001", message = "The settings are not valid", status = 500)]
    Invalid,
    #[error(code = "SHELL_002", message = "A file could not be read", status = 500)]
    Unreadable,
}

#[cfg(test)]
mod tests {
    #[derive(Error)]
    enum Shed {
        #[error(code = "TEST_SHELL_002", message = "rate limited")]
        Limited,
    }
}
"#,
    );
    write(
        root.path(),
        "tests/support/mod.rs",
        "#[derive(Error)] enum Fake { #[error(code = \"TEST_SHELL_001\", message = \"refused\")] Refused }\n",
    );

    let report = check(root.path()).expect("the messages are read");

    assert_eq!(report.undeclared, ["error.shell_002"]);
    assert!(!report.is_complete());
    assert!(report.to_string().contains("error.shell_002"), "{report}");
}

#[test]
fn a_crate_whose_every_message_is_said_in_every_language_is_complete() {
    let root = tempfile::tempdir().expect("a folder");
    write(
        root.path(),
        "locales/en/errors.mf2",
        "@locale en\n---\n[error]\nshell_001 = The settings are not valid\n",
    );
    write(
        root.path(),
        "locales/nl/errors.mf2",
        "@locale nl\n---\n[error]\nshell_001 = De instellingen kloppen niet\n",
    );
    write(
        root.path(),
        "src/lib.rs",
        "#[derive(Error)] enum E { #[error(code = \"SHELL_001\", message = \"x\")] A }\n",
    );

    let report = check(root.path()).expect("the messages are read");

    assert!(report.is_complete(), "{report}");
}

/// A library whose `label` messages an app words its own way: `area`, and `rows`, said with a whole
/// number and a text.
fn library_and_app(root: &Path) -> std::path::PathBuf {
    write(
        root,
        "lib/Cargo.toml",
        "[package]\nname = \"fixture_lib\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    );
    write(root, "lib/src/lib.rs", "");
    write(
        root,
        "lib/locales/en/label.mf2",
        "@locale en\n---\n[label]\narea = Area\nrows = .input {$count :integer} {{{$count} {$noun}}}\n",
    );
    write(
        root,
        "app/Cargo.toml",
        "[package]\nname = \"fixture_app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [dependencies]\nfixture_lib = { path = \"../lib\" }\n\n[workspace]\n",
    );
    write(
        root,
        "app/src/lib.rs",
        "fundamental_messages::messages!();\n",
    );
    root.join("app")
}

/// An app's own message under the id of a message a crate it depends on declares is that
/// message worded the app's way: the crate says it, so it is used though the app's code never
/// names it.
#[test]
fn an_app_s_wording_of_a_crate_s_message_is_used() {
    let root = tempfile::tempdir().expect("a folder");
    let app = library_and_app(root.path());
    write(
        &app,
        "locales/en/label.mf2",
        "@locale en\n---\n[label]\narea = Overview\nrows = {$count} {$noun}\n",
    );

    let report = check(&app).expect("the messages are read");

    assert!(report.is_complete(), "{report}");
}

/// A wording of a message its crate no longer declares (renamed, or gone) is said by no one,
/// even when the app's tests name the crate's own module of that name.
#[test]
fn a_wording_of_a_message_no_crate_declares_is_unused() {
    let root = tempfile::tempdir().expect("a folder");
    let app = library_and_app(root.path());
    write(
        &app,
        "locales/en/label.mf2",
        "@locale en\n---\n[label]\narea = Overview\nmenu = Menu\n",
    );
    write(
        &app,
        "tests/words.rs",
        "#[test] fn area() { assert_eq!(said(fixture_lib::m::label::area()), \"Overview\"); }\n",
    );

    let report = check(&app).expect("the messages are read");

    assert_eq!(report.unused, ["label.menu"]);
}

/// A wording may leave out what the crate's message is said with, but an input the crate never
/// gives, or one it gives as another kind of value, would leave a hole: it is mismatched.
#[test]
fn a_wording_with_an_input_its_message_is_not_said_with_is_mismatched() {
    let root = tempfile::tempdir().expect("a folder");
    let app = library_and_app(root.path());
    write(
        &app,
        "locales/en/label.mf2",
        "@locale en\n---\n[label]\narea = {$name}\nrows = .input {$noun :integer} {{{$count} {$noun}}}\n",
    );

    let report = check(&app).expect("the messages are read");

    assert_eq!(report.mismatched, ["label.area", "label.rows"]);
    assert!(report.unused.is_empty(), "{report}");
    assert!(!report.is_complete());
    assert!(
        report.to_string().contains("mismatched in en: label.area"),
        "{report}"
    );
}

/// A wording the app's own code says, by its id or through its own module, is still the crate's
/// message worded the app's way: an input the crate never gives is mismatched all the same.
#[test]
fn a_wording_the_app_says_itself_is_still_held_to_its_message() {
    let root = tempfile::tempdir().expect("a folder");
    let app = library_and_app(root.path());
    write(
        &app,
        "locales/en/label.mf2",
        "@locale en\n---\n[label]\narea = {$name}\nrows = {$count} {$noun}\n",
    );
    write(
        &app,
        "src/lib.rs",
        "fundamental_messages::messages!();\npub const AREA: &str = \"label.area\";\n",
    );

    let report = check(&app).expect("the messages are read");

    assert_eq!(report.mismatched, ["label.area"]);
    assert!(report.unused.is_empty(), "{report}");
}
