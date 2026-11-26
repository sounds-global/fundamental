//! `fun add` and `fun migrate`, driven through the binary inside a `fun new` project.

// Only `manifest_text` is used here; the cargo helpers serve the other suites.
#[allow(dead_code)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn base_project() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("app");
    let status = Command::new(env!("CARGO_BIN_EXE_fun"))
        .arg("new")
        .arg("app")
        .arg(&root)
        .args(["--surfaces", "worker"])
        .status()
        .expect("spawn fun new");
    assert!(status.success(), "`fun new` failed: {status:?}");

    // A comment, so the tests can prove edits preserve formatting.
    let manifest = root.join("fun.toml");
    let mut body = fs::read_to_string(&manifest).unwrap();
    body.insert_str(0, "# hand-written comment, must survive edits\n");
    fs::write(&manifest, body).unwrap();

    (tmp, root)
}

fn run_in(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fun"))
        .args(args)
        // Offline: a generated project's `fundamental` is a git-over-SSH dependency cargo would fetch.
        .env("CARGO_NET_OFFLINE", "true")
        .current_dir(root)
        .output()
        .expect("spawn fun")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn add_app_registers_crate_toml_and_member_preserving_comments() {
    let (_tmp, root) = base_project();
    let out = run_in(&root, &["add", "app", "billing", "--surfaces", "worker"]);
    assert!(out.status.success(), "add app failed: {out:?}");

    let manifest = common::manifest_text(&root);
    assert!(manifest.contains("[apps.billing]"), "{manifest}");
    assert!(manifest.contains("worker = {}"), "{manifest}");

    assert!(
        manifest.contains("# hand-written comment, must survive edits"),
        "comment dropped: {manifest}"
    );
    assert!(
        manifest.contains("[apps.main]"),
        "main app dropped: {manifest}"
    );

    assert!(root.join("apps/billing/Cargo.toml").exists());
    assert!(root.join("apps/billing/src/main.rs").exists());
    let app_toml = read(&root.join("apps/billing/Cargo.toml"));
    assert!(app_toml.contains("name = \"billing\""), "{app_toml}");

    let workspace = read(&root.join("Cargo.toml"));
    assert!(workspace.contains("apps/main"), "{workspace}");
    assert!(workspace.contains("apps/billing"), "{workspace}");
}

#[test]
fn add_app_wires_a_database_in_the_manifest_and_cargo_toml() {
    let (_tmp, root) = base_project();
    let out = run_in(
        &root,
        &[
            "add",
            "app",
            "billing",
            "--surfaces",
            "api",
            "--db",
            "sql",
            "--yes",
        ],
    );
    assert!(out.status.success(), "add app failed: {out:?}");

    let manifest = common::manifest_text(&root);
    assert!(
        manifest.contains("[apps.billing.database.sql]"),
        "no database block: {manifest}"
    );
    assert!(
        manifest.contains("postgresql://yugabyte@localhost:5433/billing?sslmode=disable"),
        "no default url: {manifest}"
    );

    let app_toml = read(&root.join("apps/billing/Cargo.toml"));
    assert!(
        app_toml.contains("\"sql\""),
        "sql feature missing: {app_toml}"
    );
}

#[test]
fn add_app_wires_cql_when_requested() {
    let (_tmp, root) = base_project();
    let out = run_in(
        &root,
        &[
            "add",
            "app",
            "events",
            "--surfaces",
            "api",
            "--db",
            "cql",
            "--yes",
        ],
    );
    assert!(out.status.success(), "add app failed: {out:?}");

    let manifest = common::manifest_text(&root);
    assert!(
        manifest.contains("[apps.events.database.cql]"),
        "{manifest}"
    );
    assert!(
        manifest.contains("hosts = [\"127.0.0.1:9042\"]"),
        "{manifest}"
    );
    let app_toml = read(&root.join("apps/events/Cargo.toml"));
    assert!(app_toml.contains("\"cql\""), "{app_toml}");
}

#[test]
fn add_app_registers_native_and_its_hooks_members() {
    let (_tmp, root) = base_project();
    let out = run_in(
        &root,
        &["add", "app", "desk", "--surfaces", "native", "--yes"],
    );
    assert!(out.status.success(), "add app failed: {out:?}");

    assert!(root.join("apps/desk/native/Cargo.toml").exists());
    let workspace = read(&root.join("Cargo.toml"));
    assert!(workspace.contains("apps/desk/native\""), "{workspace}");
    assert!(workspace.contains("apps/desk/native/hooks"), "{workspace}");
}

#[test]
fn add_app_rejects_an_existing_app() {
    let (_tmp, root) = base_project();
    let out = run_in(&root, &["add", "app", "main"]);
    assert!(!out.status.success(), "adding an existing app must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CLI_010"),
        "expected AlreadyExists code: {stderr}"
    );
}

#[test]
fn add_surface_extends_an_app_and_prints_guidance() {
    let (_tmp, root) = base_project();
    let out = run_in(&root, &["add", "surface", "socket"]);
    assert!(out.status.success(), "add surface failed: {out:?}");

    let manifest = common::manifest_text(&root);
    assert!(manifest.contains("socket = { port = 9000 }"), "{manifest}");

    let lib = fs::read_to_string(root.join("apps/main/src/lib.rs")).expect("the library");
    assert!(
        lib.contains(
            "    fundamental::app!()\n        .socket(|router| router.route(on_ping_route()))\n"
        ),
        "{lib}"
    );
    assert!(lib.contains("async fn on_ping("), "{lib}");
    let app_toml = fs::read_to_string(root.join("apps/main/Cargo.toml")).expect("the manifest");
    assert!(app_toml.contains("\"ws\""), "{app_toml}");
    let main = fs::read_to_string(root.join("apps/main/src/main.rs")).expect("the binary");
    assert!(main.contains("main::app().run()"), "{main}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("apps/main/src/lib.rs"), "{stdout}");
}

#[test]
fn add_surface_prints_the_line_for_an_app_it_cannot_find() {
    let (_tmp, root) = base_project();
    fs::write(root.join("apps/main/src/lib.rs"), "//! mine\n").expect("the library");
    let out = run_in(&root, &["add", "surface", "api"]);
    assert!(out.status.success(), "add surface failed: {out:?}");

    assert_eq!(
        fs::read_to_string(root.join("apps/main/src/lib.rs")).expect("kept"),
        "//! mine\n"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(".api(register_api)"), "{stdout}");
}

#[test]
fn add_surface_rejects_a_duplicate_surface() {
    let (_tmp, root) = base_project();
    let out = run_in(&root, &["add", "surface", "worker"]);
    assert!(!out.status.success(), "duplicate surface must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("CLI_010"), "{stderr}");
}

#[test]
fn add_migrations_drops_a_first_pair() {
    let (_tmp, root) = base_project();
    let out = run_in(&root, &["add", "migrations", "--engine", "sql"]);
    assert!(out.status.success(), "add migrations failed: {out:?}");

    assert!(
        root.join("apps/main/migrations/sql/0001_init.up.sql")
            .exists()
    );
    assert!(
        root.join("apps/main/migrations/sql/0001_init.down.sql")
            .exists()
    );
}

#[test]
fn add_seeds_creates_a_sample_seed() {
    let (_tmp, root) = base_project();
    let out = run_in(&root, &["add", "seeds"]);
    assert!(out.status.success(), "add seeds failed: {out:?}");

    let example = root.join("apps/main/seeds/example.rs");
    assert!(example.exists(), "seeds/example.rs missing");
    assert!(read(&example).contains("#[seed("), "no #[seed] stub");
    assert!(root.join("apps/main/seeds/README.md").exists());
}

#[test]
fn migrate_create_writes_a_timestamped_pair() {
    let (_tmp, root) = base_project();
    let out = run_in(
        &root,
        &["migrate", "create", "create_users", "--engine", "sql"],
    );
    assert!(out.status.success(), "migrate create failed: {out:?}");

    let dir = root.join("apps/main/migrations/sql");
    let ups: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with("_create_users.up.sql"))
        .collect();
    assert_eq!(ups.len(), 1, "expected one up file, got {ups:?}");
    let stem = ups[0].trim_end_matches(".up.sql");
    assert!(
        dir.join(format!("{stem}.down.sql")).exists(),
        "missing matching down for {stem}"
    );
}

#[test]
fn migrate_list_reports_created_migrations() {
    let (_tmp, root) = base_project();
    assert!(
        run_in(&root, &["migrate", "create", "create_users"])
            .status
            .success()
    );
    assert!(
        run_in(&root, &["migrate", "create", "add_index"])
            .status
            .success()
    );

    let out = run_in(&root, &["migrate", "list"]);
    assert!(out.status.success(), "migrate list failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("create_users"), "{stdout}");
    assert!(stdout.contains("add_index"), "{stdout}");
}

#[test]
fn migrate_list_is_friendly_when_empty() {
    let (_tmp, root) = base_project();
    let out = run_in(&root, &["migrate", "list"]);
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("no migrations found"), "{stdout}");
}

#[test]
fn commands_outside_a_project_error_clearly() {
    let tmp = TempDir::new().unwrap();
    let out = run_in(tmp.path(), &["migrate", "list"]);
    assert!(!out.status.success(), "must fail outside a project");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CLI_008"),
        "expected NotInProject code: {stderr}"
    );
}
