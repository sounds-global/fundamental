//! A message calls only the functions the runtime has: `:string`, `:integer`, `:number`,
//! `:percent`, `:currency` and `:brand`, the last with a literal word of the brand. Any other
//! call fails the build and the check, naming the message, rather than a sentence that says
//! nothing when it is read.

use std::path::Path;

use fundamental_messages_build::{Environment, Policy, build, check, signatures};

fn resource(value: &str) -> String {
    format!("@locale en\n---\n[mail]\nsigned = {value}\n")
}

fn refused(value: &str) -> Vec<String> {
    signatures(&resource(value)).expect_err("a refused call")
}

#[test]
fn the_runtime_s_functions_and_each_word_of_the_brand_are_called() {
    let found = signatures(&resource(
        ".input {$n :integer} {{{$n :number} {$n :percent} {$s :string} {name :brand} \
         {legal-name :brand} {site-domain :brand} {platform-domain :brand}}}",
    ))
    .expect("every call is the runtime's");
    let names: Vec<&str> = found[0]
        .inputs
        .iter()
        .map(|input| input.name.as_str())
        .collect();
    assert_eq!(names, ["n", "s"], ":brand adds no input");
}

#[test]
fn a_function_the_runtime_lacks_is_refused_naming_the_message() {
    let problems = refused("Sent {$x :frobnicate}");
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("mail.signed"), "{problems:?}");
    assert!(problems[0].contains(":frobnicate"), "{problems:?}");
}

#[test]
fn a_function_mf2_knows_but_the_runtime_lacks_is_refused() {
    assert!(refused("On {$at :datetime}")[0].contains(":datetime"));
    assert!(refused(".local $d = {$at :date} {{On {$d}}}")[0].contains(":date"));
}

#[test]
fn a_misspelt_word_of_the_brand_is_refused() {
    let problems = refused("Sent by {nme :brand}");
    assert!(problems[0].contains("mail.signed"), "{problems:?}");
    assert!(problems[0].contains("nme"), "{problems:?}");
}

#[test]
fn the_brand_from_a_variable_is_refused() {
    assert!(refused("Sent by {$who :brand}")[0].contains(":brand"));
}

#[test]
fn the_brand_without_a_word_is_refused() {
    assert!(refused("Sent by {:brand}")[0].contains(":brand"));
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("folder");
    std::fs::write(path, text).expect("written");
}

#[test]
fn a_translation_s_call_the_runtime_lacks_fails_the_build_and_the_check() {
    let root = tempfile::tempdir().expect("a folder");
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname = \"calls\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[lib]\npath = \"lib.rs\"\n",
    );
    write(root.path(), "lib.rs", "");
    write(root.path(), "locales/en/mail.mf2", &resource("On {$at}"));
    write(
        root.path(),
        "locales/nl/mail.mf2",
        "@locale nl\n---\n[mail]\nsigned = Op {$at :datetime}\n",
    );
    let out = tempfile::tempdir().expect("a folder");
    let problems = build(
        root.path(),
        out.path(),
        &Policy::default_for(Environment::Local),
    )
    .expect_err("the build fails");
    assert!(
        problems.iter().any(|problem| problem.contains("nl")
            && problem.contains("mail.signed")
            && problem.contains(":datetime")),
        "{problems:?}"
    );
    let problems = check(root.path()).expect_err("the check fails");
    assert!(
        problems.iter().any(|problem| problem.contains(":datetime")),
        "{problems:?}"
    );
}

#[test]
fn the_build_knows_the_runtime_s_functions_and_brand_words() {
    assert_eq!(
        fundamental_messages_build::FUNCTIONS,
        fundamental_messages::FUNCTION_NAMES
    );
    let words: Vec<&str> = fundamental_messages::BrandTerm::ALL
        .iter()
        .map(|term| term.name())
        .collect();
    assert_eq!(fundamental_messages_build::BRAND_TERMS.as_slice(), words);
}
