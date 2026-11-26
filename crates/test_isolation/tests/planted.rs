//! Each isolation rule refuses a planted test, and admits it once it runs alone, is confined, or is a relay.

use std::path::{Path, PathBuf};

use fundamental_test_isolation::{IsolationRule, RunAlone, TestIsolation};

const PLANTED: &str = r#"
#[tokio::test]
async fn leases_beside_others() {
    let (connections, steward) = open_steward(&app).await;
    connections.close().await;
}

#[tokio::test]
async fn leases_and_never_revokes() {
    let connections = SqlConnections::from_settings(&settings, env, Some(&vault)).await;
}

#[tokio::test]
async fn changes_the_schema() {
    stack.ysql_as("yugabyte", password, &[], include_str!("fixtures/grant.sql"));
}

#[tokio::test]
async fn drains_everything() {
    JobsDrain::<Kind, _>::new(pool, handler, settings).drain(100).await;
}

#[tokio::test]
async fn drains_its_own() {
    JobsDrain::<Kind, _>::new(pool, handler, settings)
        .confined_to(JobConfinement::naming(team)?)
        .drain(100)
        .await;
}
"#;

/// A test binary and its fixture in a directory of their own.
fn planted() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("a directory");
    let tests = dir.path().join("tests");
    std::fs::create_dir_all(tests.join("fixtures")).expect("the fixtures");
    std::fs::write(
        tests.join("fixtures/grant.sql"),
        "-- a right the test proves\nGRANT SELECT ON t TO r;\n",
    )
    .expect("a fixture");
    let file = tests.join("planted.rs");
    std::fs::write(&file, PLANTED).expect("the source");
    (dir, file)
}

fn rules(check: &TestIsolation, file: &Path) -> Vec<(String, IsolationRule)> {
    let source = std::fs::read_to_string(file).expect("the source");
    let mut found: Vec<(String, IsolationRule)> = check
        .check(file, &source)
        .expect("a readable source")
        .into_iter()
        .map(|finding| (finding.function, finding.rule))
        .collect();
    found.sort();
    found
}

#[test]
fn each_way_beyond_its_own_rows_is_refused() {
    let (_dir, file) = planted();
    let check = TestIsolation::new(RunAlone::default()).leasing(["open_steward"]);
    assert_eq!(
        rules(&check, &file),
        [
            ("changes_the_schema".to_owned(), IsolationRule::SchemaChange),
            (
                "drains_everything".to_owned(),
                IsolationRule::UnconfinedDrain
            ),
            (
                "leases_and_never_revokes".to_owned(),
                IsolationRule::LeaseBesideOthers
            ),
            (
                "leases_and_never_revokes".to_owned(),
                IsolationRule::LeaseNotRevoked
            ),
            (
                "leases_beside_others".to_owned(),
                IsolationRule::LeaseBesideOthers
            ),
        ]
    );
}

#[test]
fn a_test_run_alone_may_lease_drain_and_change_the_schema_but_still_revokes() {
    let (_dir, file) = planted();
    let alone = RunAlone::from_nextest(
        "[[profile.default.overrides]]\nfilter = 'test(=leases_beside_others) | test(=leases_and_never_revokes) | test(=changes_the_schema) | test(=drains_everything)'\nthreads-required = \"num-test-threads\"\n",
    )
    .expect("a readable list");
    let check = TestIsolation::new(alone).leasing(["open_steward"]);
    assert_eq!(
        rules(&check, &file),
        [(
            "leases_and_never_revokes".to_owned(),
            IsolationRule::LeaseNotRevoked
        )]
    );
}

#[test]
fn a_project_names_its_own_revoke() {
    let (_dir, file) = planted();
    let source = std::fs::read_to_string(&file).expect("the source").replace(
        "Some(&vault)).await;\n}",
        "Some(&vault)).await;\n    connections.revoke(&vault).await;\n}",
    );
    std::fs::write(&file, source).expect("the source");
    let check = TestIsolation::new(RunAlone::default())
        .leasing(["open_steward"])
        .revoking(["revoke"]);
    assert!(
        !rules(&check, &file)
            .iter()
            .any(|(_, rule)| *rule == IsolationRule::LeaseNotRevoked)
    );
}

#[test]
fn a_named_relay_may_drain_everything() {
    let (_dir, file) = planted();
    let check = TestIsolation::new(RunAlone::default())
        .leasing(["open_steward"])
        .relays([("tests/planted.rs", "drains_everything")]);
    assert!(
        !rules(&check, &file)
            .iter()
            .any(|(_, rule)| *rule == IsolationRule::UnconfinedDrain)
    );
}

#[test]
fn a_relay_of_the_same_name_in_another_file_is_not_admitted() {
    let (_dir, file) = planted();
    let check = TestIsolation::new(RunAlone::default())
        .leasing(["open_steward"])
        .relays([("tests/events.rs", "drains_everything")]);
    assert!(
        rules(&check, &file)
            .iter()
            .any(|(_, rule)| *rule == IsolationRule::UnconfinedDrain)
    );
}

#[test]
fn closing_something_else_revokes_nothing() {
    let check = TestIsolation::new(RunAlone::default()).leasing(["open_steward"]);
    let found = check
        .check(
            Path::new("tests/closing.rs"),
            "async fn t() { let (connections, steward) = open_steward(&app).await; socket.close().await; } \
             async fn u() { open_steward(&app).await.unwrap(); } \
             async fn v() { let (connections, _) = open_steward(&app).await; connections.close().await; } \
             async fn w() { let refused = open_steward(&app).await; if let Ok(lease) = &refused { lease.close().await; } }",
        )
        .expect("a readable source");
    let unrevoked: Vec<&str> = found
        .iter()
        .filter(|finding| finding.rule == IsolationRule::LeaseNotRevoked)
        .map(|finding| finding.function.as_str())
        .collect();
    assert_eq!(
        unrevoked,
        ["t", "u"],
        "a lease bound to nothing cannot be revoked"
    );
}

#[test]
fn a_finding_names_its_code_place_and_remedy() {
    let (_dir, file) = planted();
    let source = std::fs::read_to_string(&file).expect("the source");
    let check = TestIsolation::new(RunAlone::default());
    let drained = check
        .check(&file, &source)
        .expect("a readable source")
        .into_iter()
        .find(|finding| finding.rule == IsolationRule::UnconfinedDrain)
        .expect("the unconfined drain");
    let said = drained.to_string();
    assert!(said.starts_with("ISO_004  "), "{said}");
    assert!(said.contains("planted.rs:"), "{said}");
    assert!(said.contains("`drains_everything`"), "{said}");
    assert!(
        said.contains(IsolationRule::UnconfinedDrain.remedy()),
        "{said}"
    );
}

#[test]
fn a_fixture_that_is_missing_is_an_error_not_a_pass() {
    let (dir, file) = planted();
    std::fs::remove_file(dir.path().join("tests/fixtures/grant.sql")).expect("removed");
    let source = std::fs::read_to_string(&file).expect("the source");
    let refused = TestIsolation::new(RunAlone::default())
        .check(&file, &source)
        .expect_err("an unread fixture is not a fixture without DDL");
    assert_eq!(fundamental_core::Error::error_code(&refused), "ISO_102");
}

#[test]
fn a_source_that_is_not_rust_is_an_error() {
    let refused = TestIsolation::new(RunAlone::default())
        .check(Path::new("tests/broken.rs"), "fn (")
        .expect_err("not Rust");
    assert_eq!(fundamental_core::Error::error_code(&refused), "ISO_101");
}

const WAITS: &str = r#"
#[tokio::test]
async fn sleeps_a_second() {
    tokio::time::sleep(Duration::from_secs(1)).await;
}

#[tokio::test]
async fn holds_the_race_open() {
    tokio::time::sleep(support::RACE_WINDOW).await;
}

#[tokio::test]
async fn drops_what_it_waited_for() {
    let _ = eventually("sealed", WITHIN, tokio::time::sleep, probe).await;
}

#[tokio::test]
async fn keeps_what_it_waited_for() {
    eventually("sealed", WITHIN, tokio::time::sleep, probe).await.unwrap();
}
"#;

#[test]
fn a_fixed_sleep_and_a_dropped_wait_are_refused_and_the_race_window_is_not() {
    let dir = tempfile::tempdir().expect("a directory");
    let file = dir.path().join("tests/waits.rs");
    std::fs::create_dir_all(dir.path().join("tests")).expect("the tests");
    std::fs::write(&file, WAITS).expect("the source");
    let check = TestIsolation::new(RunAlone::default()).race_windows(["RACE_WINDOW"]);
    assert_eq!(
        rules(&check, &file),
        [
            (
                "drops_what_it_waited_for".to_owned(),
                IsolationRule::DiscardedWait
            ),
            ("sleeps_a_second".to_owned(), IsolationRule::FixedSleep),
        ]
    );
    let without_window = TestIsolation::new(RunAlone::default());
    assert!(
        rules(&without_window, &file)
            .contains(&("holds_the_race_open".to_owned(), IsolationRule::FixedSleep)),
        "a race window is admitted only once the project names it"
    );
}
