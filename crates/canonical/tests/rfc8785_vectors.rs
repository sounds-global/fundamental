//! The RFC author's own test data, read and re-written byte for byte.

use fundamental_canonical::{
    Limits, Profile, Sha256Digest, from_canonical_slice, to_canonical_vec, validate_canonical,
};
use std::path::{Path, PathBuf};

/// The vectors inside this crate's integer profile; `values` carries floats.
const IN_PROFILE: [&str; 5] = ["arrays", "french", "structures", "unicode", "weird"];

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/rfc8785")
}

fn read(relative: &str) -> Vec<u8> {
    let path = data().join(relative);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn the_vendored_files_are_the_pinned_upstream_bytes() {
    let sums = String::from_utf8(read("SHA256SUMS")).expect("SHA256SUMS is text");
    let mut checked = 0;
    for line in sums.lines() {
        let (expected, name) = line.split_once("  ").expect("a sha256sum line");
        assert_eq!(
            hex(Sha256Digest::of(&read(name)).as_bytes()),
            expected,
            "{name} changed"
        );
        checked += 1;
    }
    assert_eq!(checked, 13, "LICENSE and six input and six output files");
}

#[test]
fn every_in_profile_output_is_accepted_and_rewritten_byte_for_byte() {
    for name in IN_PROFILE {
        let canonical = read(&format!("output/{name}.json"));
        let value: serde_json::Value =
            from_canonical_slice(&canonical, &Limits::DOCUMENT, Profile::Verifier)
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(
            to_canonical_vec(&value).ok().as_deref(),
            Some(canonical.as_slice()),
            "{name}"
        );
    }
}

#[test]
fn every_in_profile_input_canonicalises_to_its_output() {
    // `structures` spells 56 as `56.0`, a float this profile refuses.
    for name in IN_PROFILE.iter().filter(|name| **name != "structures") {
        let lenient: serde_json::Value =
            serde_json::from_slice(&read(&format!("input/{name}.json")))
                .expect("the upstream input parses");
        assert_eq!(
            to_canonical_vec(&lenient).ok(),
            Some(read(&format!("output/{name}.json"))),
            "{name}"
        );
    }
}

#[test]
fn every_input_is_refused_as_it_stands() {
    for name in IN_PROFILE.iter().chain(["values"].iter()) {
        let input = read(&format!("input/{name}.json"));
        assert!(
            validate_canonical(&input, &Limits::DOCUMENT, Profile::Verifier).is_err(),
            "{name}'s pretty-printed input was accepted"
        );
    }
}

#[test]
fn a_float_spelling_of_an_integer_is_not_written() {
    let lenient: serde_json::Value =
        serde_json::from_slice(&read("input/structures.json")).expect("the upstream input parses");
    assert_eq!(
        to_canonical_vec(&lenient).map_err(|error| error.rule()),
        Err("unrepresentable")
    );
}

#[test]
fn the_float_vector_is_outside_the_profile() {
    let error = validate_canonical(
        &read("output/values.json"),
        &Limits::DOCUMENT,
        Profile::Verifier,
    )
    .expect_err("values.json carries floats");
    assert_eq!(error.rule(), "float");
}

#[cfg(feature = "registry")]
#[test]
fn the_unnormalised_vector_is_refused_by_the_registry_profile() {
    let error = validate_canonical(
        &read("output/unicode.json"),
        &Limits::DOCUMENT,
        Profile::Registry,
    )
    .expect_err("unicode.json is canonical and not NFC");
    assert_eq!(error.rule(), "not_nfc");
}
