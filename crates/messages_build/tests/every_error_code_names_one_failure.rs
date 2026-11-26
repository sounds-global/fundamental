//! No two failures in the framework share an error code.

use fundamental_messages_build::ErrorCodeIndex;
use std::path::Path;

#[test]
fn every_error_code_names_one_failure() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let index = ErrorCodeIndex::scan(&[&workspace.join("crates"), &workspace.join("cli")])
        .expect("the framework's sources are readable");
    assert!(
        index.len() > 100,
        "the scan found {} codes; it is not looking where the errors are",
        index.len()
    );
    assert!(
        index.unreadable().is_empty(),
        "unreadable, so unchecked: {:?}",
        index.unreadable()
    );
    let report: Vec<String> = index
        .duplicates()
        .into_iter()
        .map(|(code, sites)| {
            let sites: Vec<String> = sites.iter().map(ToString::to_string).collect();
            format!("{code}: {}", sites.join(", "))
        })
        .collect();
    assert!(
        report.is_empty(),
        "one code, several failures:\n{}",
        report.join("\n")
    );
}
