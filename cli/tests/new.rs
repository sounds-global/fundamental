//! `fun new`, driven non-interactively through the binary; the build tests compile what it writes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

mod common;
use common::cargo_in_generated;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli is nested under the repo root")
        .to_path_buf()
}

fn run_new(name: &str, path: &Path, extra: &[&str]) {
    let status = Command::new(env!("CARGO_BIN_EXE_fun"))
        .env("CARGO_NET_OFFLINE", "true")
        .arg("new")
        .arg(name)
        .arg(path)
        .args(extra)
        .status()
        .expect("spawn fun");
    assert!(status.success(), "`fun new` exited non-zero: {status:?}");
}

/// Piped, so `console` emits plain text without ANSI styling.
fn run_new_capture(name: &str, path: &Path, extra: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_fun"))
        .env("CARGO_NET_OFFLINE", "true")
        .arg("new")
        .arg(name)
        .arg(path)
        .args(extra)
        .output()
        .expect("spawn fun");
    assert!(out.status.success(), "`fun new` exited non-zero: {out:?}");
    String::from_utf8(out.stdout).expect("stdout is utf-8")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn domain_only_emits_a_library_workspace_and_no_app() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("acme");
    run_new("acme", &root, &["--yes"]);

    for f in [
        "fun.toml",
        "Cargo.toml",
        ".gitignore",
        "README.md",
        "domain/Cargo.toml",
        "domain/src/lib.rs",
    ] {
        assert!(root.join(f).exists(), "missing {f}");
    }

    assert!(
        !root.join("apps").exists(),
        "domain-only must not emit apps/"
    );

    let toml = common::manifest_text(&root);
    assert!(
        !toml.contains("[apps"),
        "domain-only must have no [apps]: {toml}"
    );
    assert!(toml.contains("name = \"acme\""));

    let workspace = read(&root.join("Cargo.toml"));
    assert!(workspace.contains("members = [\"domain\"]"), "{workspace}");

    let domain_toml = read(&root.join("domain/Cargo.toml"));
    assert!(domain_toml.contains("name = \"domain\""), "{domain_toml}");
    assert!(domain_toml.contains("edition = \"2024\""));
}

#[test]
fn new_scaffolds_parallel_tests_and_optimised_test_dependencies() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("acme");
    run_new("acme", &root, &["--yes"]);

    let nextest: toml::Table = read(&root.join(".config/nextest.toml")).parse().unwrap();
    assert_eq!(
        nextest["profile"]["default"]["test-threads"].as_str(),
        Some("num-cpus")
    );
    let workspace: toml::Table = read(&root.join("Cargo.toml")).parse().unwrap();
    assert_eq!(
        workspace["profile"]["dev"]["package"]["*"]["opt-level"].as_integer(),
        Some(2)
    );
}

#[test]
fn new_writes_one_settings_file_and_ignores_the_machines() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("acme");
    run_new("acme", &root, &["--surfaces", "worker"]);

    let settings = read(&root.join("fun.toml"));
    assert!(
        settings.starts_with("[project]\nname = \"acme\""),
        "{settings}"
    );
    assert!(settings.contains("[apps.main]\nworker = {}"), "{settings}");
    assert!(
        !settings
            .lines()
            .any(|line| line.trim_start().starts_with('#')),
        "settings are values; what they mean is in the docs: {settings}"
    );
    assert!(!root.join("fundamental").exists(), "no settings folder");
    assert!(!root.join("config").exists(), "no config folder");
    let ignored = read(&root.join(".gitignore"));
    assert!(
        ignored.lines().any(|line| line == "/fun.local.toml"),
        "{ignored}"
    );
    let manifest =
        fundamental_manifest::Manifest::load(&root).expect("the generated settings load");
    assert_eq!(manifest.files(), [std::path::PathBuf::from("fun.toml")]);
}

#[test]
fn the_app_defaults_to_main() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("acme");
    run_new("acme", &root, &["--surfaces", "worker"]);

    let app_toml = read(&root.join("apps/main/Cargo.toml"));
    assert!(app_toml.contains("name = \"main\""), "{app_toml}");

    let main = read(&root.join("apps/main/src/main.rs"));
    assert!(main.contains("main::app().run()"), "{main}");

    let fundamental = common::manifest_text(&root);
    assert!(fundamental.contains("[apps.main]"), "{fundamental}");
    assert!(fundamental.contains("name = \"acme\""), "{fundamental}");
}

#[test]
fn a_custom_app_name_sanitizes_its_crate_and_threads_everywhere() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("acme");
    run_new("acme", &root, &["--surfaces", "worker", "--app", "my-svc"]);

    let app_toml = read(&root.join("apps/my-svc/Cargo.toml"));
    assert!(app_toml.contains("name = \"my_svc\""), "{app_toml}");

    let main = read(&root.join("apps/my-svc/src/main.rs"));
    assert!(main.contains("my_svc::app().run()"), "{main}");
    let lib = read(&root.join("apps/my-svc/src/lib.rs"));
    assert!(lib.contains("fundamental::app!()"), "{lib}");

    let fundamental = common::manifest_text(&root);
    assert!(fundamental.contains("[apps.my-svc]"), "{fundamental}");
    assert!(fundamental.contains("name = \"acme\""), "{fundamental}");

    let workspace = read(&root.join("Cargo.toml"));
    assert!(
        workspace.contains("members = [\"domain\", \"apps/my-svc\"]"),
        "{workspace}"
    );
}

#[test]
fn a_custom_socket_port_lands_in_the_manifest() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("p");
    run_new("p", &root, &["--surfaces", "socket", "--port", "8443"]);

    let toml = common::manifest_text(&root);
    assert!(toml.contains("socket = { port = 8443 }"), "{toml}");
}

#[test]
fn a_database_choice_writes_config_and_enables_the_feature() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("d");
    run_new("d", &root, &["--surfaces", "worker", "--db", "sql"]);

    let toml = common::manifest_text(&root);
    assert!(toml.contains("[apps.main.database.sql]"), "{toml}");
    assert!(toml.contains("url = "), "{toml}");

    let app_toml = read(&root.join("apps/main/Cargo.toml"));
    assert!(app_toml.contains("\"sql\""), "{app_toml}");

    assert!(
        !root.join("apps/main/migrations").exists(),
        "config-only db must not scaffold migrations/"
    );
}

#[test]
fn cql_writes_a_keyspace_stub_and_the_cql_feature() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("sc");
    run_new("sc", &root, &["--surfaces", "worker", "--db", "cql"]);

    let toml = common::manifest_text(&root);
    assert!(toml.contains("[apps.main.database.cql]"), "{toml}");
    assert!(toml.contains("keyspace = \"main\""), "{toml}");

    let app_toml = read(&root.join("apps/main/Cargo.toml"));
    assert!(app_toml.contains("\"cql\""), "{app_toml}");
}

#[test]
fn both_databases_can_be_selected_together() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("both");
    run_new("both", &root, &["--surfaces", "worker", "--db", "sql,cql"]);

    let toml = common::manifest_text(&root);
    assert!(toml.contains("[apps.main.database.sql]"), "{toml}");
    assert!(toml.contains("[apps.main.database.cql]"), "{toml}");

    let app_toml = read(&root.join("apps/main/Cargo.toml"));
    assert!(app_toml.contains("\"sql\""), "{app_toml}");
    assert!(app_toml.contains("\"cql\""), "{app_toml}");
}

#[test]
fn an_unknown_database_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("bad");
    let status = Command::new(env!("CARGO_BIN_EXE_fun"))
        .env("CARGO_NET_OFFLINE", "true")
        .arg("new")
        .arg("bad")
        .arg(&root)
        .args(["--surfaces", "worker", "--db", "mongo"])
        .status()
        .expect("spawn fun");
    assert!(!status.success(), "unknown --db must fail");
    assert!(!root.exists(), "a rejected run must not scaffold anything");
}

#[test]
fn a_missing_name_under_yes_fails() {
    let status = Command::new(env!("CARGO_BIN_EXE_fun"))
        .env("CARGO_NET_OFFLINE", "true")
        .arg("new")
        .arg("--yes")
        .status()
        .expect("spawn fun");
    assert!(!status.success(), "`fun new --yes` with no name must fail");
}

#[test]
fn worker_app_emits_a_binary_and_a_domain() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("w");
    run_new("w", &root, &["--surfaces", "worker"]);

    assert!(
        root.join("domain/Cargo.toml").exists(),
        "app project must still emit the shared domain/ crate"
    );

    let lib = read(&root.join("apps/main/src/lib.rs"));
    assert!(lib.contains(".worker(register_workers)"), "{lib}");
    assert!(
        lib.contains("fundamental::mount_workers! { on_hello }"),
        "{lib}"
    );

    let toml = common::manifest_text(&root);
    assert!(toml.contains("[apps.main]"), "{toml}");
    assert!(toml.contains("worker = {}"), "{toml}");
    assert!(!toml.contains("socket"), "{toml}");

    let app_toml = read(&root.join("apps/main/Cargo.toml"));
    assert!(app_toml.contains("\"nats\""), "{app_toml}");
    assert!(app_toml.contains("\"app\""), "{app_toml}");
    assert!(
        app_toml.contains("fundamental = { workspace = true, features = ["),
        "{app_toml}"
    );
    assert!(
        app_toml.contains("domain = { path = \"../../domain\", features = [\"server\"] }"),
        "{app_toml}"
    );

    let workspace = read(&root.join("Cargo.toml"));
    assert!(
        workspace.contains("members = [\"domain\", \"apps/main\"]"),
        "{workspace}"
    );
    assert!(
        workspace
            .contains("fundamental = { git = \"https://github.com/sounds-global/fundamental\" }"),
        "{workspace}"
    );
}

#[test]
fn socket_app_emits_a_keyed_socket_surface_with_a_port() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("s");
    run_new("s", &root, &["--surfaces", "socket"]);

    let toml = common::manifest_text(&root);
    assert!(toml.contains("[apps.main]"), "{toml}");
    assert!(toml.contains("socket = { port = 9000 }"), "{toml}");
    assert!(!toml.contains("worker"), "{toml}");

    let lib = read(&root.join("apps/main/src/lib.rs"));
    assert!(
        lib.contains(".socket(|router| router.route(on_ping_route()))"),
        "{lib}"
    );
    assert!(
        lib.contains(".socket_config(fundamental::ws::WsConfig::public)"),
        "{lib}"
    );

    let app_toml = read(&root.join("apps/main/Cargo.toml"));
    assert!(app_toml.contains("\"ws\""), "{app_toml}");
}

#[test]
fn both_surfaces_emit_worker_and_socket() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("b");
    run_new("b", &root, &["--surfaces", "worker,socket"]);

    let toml = common::manifest_text(&root);
    assert!(toml.contains("worker = {}"), "{toml}");
    assert!(toml.contains("socket = { port = 9000 }"), "{toml}");

    let main = read(&root.join("apps/main/src/main.rs"));
    assert!(main.contains("main::app().run()"), "{main}");
    let lib = read(&root.join("apps/main/src/lib.rs"));
    assert!(lib.contains(".worker(register_workers)"), "{lib}");
    assert!(
        lib.contains(".socket(|router| router.route(on_ping_route()))"),
        "{lib}"
    );
    assert!(lib.contains("subscribe as nats_subscribe"), "{lib}");
    assert!(lib.contains("subscribe as ws_subscribe"), "{lib}");

    let app_toml = read(&root.join("apps/main/Cargo.toml"));
    assert!(app_toml.contains("\"nats\""), "{app_toml}");
    assert!(app_toml.contains("\"ws\""), "{app_toml}");
    assert!(app_toml.contains("\"app\""), "{app_toml}");
}

#[test]
fn output_speaks_the_structured_bar_language() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("vis");
    let stdout = run_new_capture("vis", &root, &["--surfaces", "worker"]);

    assert!(stdout.contains("fundamental"), "{stdout}");
    assert!(stdout.contains("▌ Review"), "{stdout}");
    assert!(stdout.contains("▌ Created vis"), "{stdout}");
    assert!(stdout.contains("▌ Next steps"), "{stdout}");
    assert!(stdout.contains("→ cargo run"), "{stdout}");

    for line in stdout.lines() {
        assert!(
            !line.trim_start().starts_with('#'),
            "a note still reads as a shell comment: {line:?}"
        );
    }
    assert!(
        !stdout.contains("v Created"),
        "old fake checkmark survived: {stdout}"
    );
}

#[test]
fn quiet_silences_stdout() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("hush");
    let stdout = run_new_capture("hush", &root, &["--yes", "--quiet"]);
    assert!(
        stdout.trim().is_empty(),
        "--quiet must silence stdout: {stdout:?}"
    );
    assert!(root.join("fun.toml").exists());
}

#[test]
fn refuses_an_existing_directory() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("taken");
    fs::create_dir(&root).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_fun"))
        .env("CARGO_NET_OFFLINE", "true")
        .arg("new")
        .arg("taken")
        .arg(&root)
        .arg("--yes")
        .status()
        .expect("spawn fun");
    assert!(!status.success(), "existing dir must fail");
}

const ONE_LINE_MAIN: &str = "fn main() -> std::process::ExitCode {\n    main::app().run()\n}\n";

fn assert_runs_in_one_line(root: &Path) {
    let main = read(&root.join("apps/main/src/main.rs"));
    assert!(main.ends_with(ONE_LINE_MAIN), "{main}");
}

/// Each shape is generated in a directory of its own name, so they share one build directory.
fn assert_shape_builds_clean(surfaces: &str, extra: &[&str]) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("shapeproof");
    let mut args = vec!["--surfaces", surfaces];
    args.extend(extra);
    run_new("shapeproof", &root, &args);
    assert_runs_in_one_line(&root);
    build_against_this_checkout(&root);

    let output = cargo_in_generated(&root, &["clippy", "--all-targets", "--", "-D", "warnings"]);

    assert!(
        output.status.success(),
        "the generated {surfaces} project is not clippy-clean:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// With a database, so the domain's engine feature and `domain/server` are exercised.
#[test]
fn generated_worker_project_builds() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("buildproof");
    run_new(
        "buildproof",
        &root,
        &["--surfaces", "worker", "--db", "sql"],
    );
    assert_runs_in_one_line(&root);

    build_against_this_checkout(&root);

    let output = cargo_in_generated(&root, &["build"]);

    assert!(
        output.status.success(),
        "generated project failed to build:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn generated_web_and_worker_project_builds_clean_under_clippy() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("lintproof");
    run_new(
        "lintproof",
        &root,
        &["--surfaces", "web,worker", "--db", "sql"],
    );
    assert_runs_in_one_line(&root);
    build_against_this_checkout(&root);

    let output = cargo_in_generated(&root, &["clippy", "--all-targets", "--", "-D", "warnings"]);

    assert!(
        output.status.success(),
        "generated project is not clippy-clean:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn generated_web_project_builds_clean_under_clippy() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("siteproof");
    run_new("siteproof", &root, &["--surfaces", "web"]);
    assert_runs_in_one_line(&root);
    build_against_this_checkout(&root);

    let output = cargo_in_generated(&root, &["clippy", "--all-targets", "--", "-D", "warnings"]);

    assert!(
        output.status.success(),
        "generated web project is not clippy-clean:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn generated_api_project_builds() {
    assert_shape_builds_clean("api", &[]);
}

#[test]
fn generated_socket_project_builds() {
    assert_shape_builds_clean("socket", &[]);
}

#[test]
fn generated_sync_project_builds() {
    assert_shape_builds_clean("sync", &["--db", "sql"]);
}

#[test]
fn generated_web_and_api_project_builds() {
    assert_shape_builds_clean("web,api", &[]);
}

#[test]
fn generated_web_worker_and_socket_project_builds() {
    assert_shape_builds_clean("web,worker,socket", &[]);
}

#[test]
fn generated_every_surface_project_builds() {
    assert_shape_builds_clean("web,api,worker,socket,sync", &["--db", "sql"]);
}

#[test]
fn add_surface_then_builds() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("shapeproof");
    run_new("shapeproof", &root, &["--surfaces", "worker"]);
    for kind in ["socket", "api"] {
        let added = fun_in(&root, &["add", "surface", kind]);
        assert!(
            added.status.success(),
            "fun add surface {kind} failed:\n{}",
            String::from_utf8_lossy(&added.stderr)
        );
    }
    assert_runs_in_one_line(&root);
    let lib = read(&root.join("apps/main/src/lib.rs"));
    assert!(
        lib.contains(".socket(|router| router.route(on_ping_route()))"),
        "{lib}"
    );
    assert!(lib.contains(".api(register_api)"), "{lib}");
    build_against_this_checkout(&root);

    let output = cargo_in_generated(&root, &["clippy", "--all-targets", "--", "-D", "warnings"]);

    assert!(
        output.status.success(),
        "the app with added surfaces is not clippy-clean:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Points the git sources at this checkout; `sqlx-sqlite` at its stub directory, `fundamental_build` at its own.
fn build_against_this_checkout(root: &Path) {
    let git = "https://github.com/sounds-global/fundamental";
    let repo = repo_root();
    let mut manifests = vec![root.join("Cargo.toml")];
    if let Ok(apps) = fs::read_dir(root.join("apps")) {
        manifests.extend(apps.flatten().map(|app| app.path().join("Cargo.toml")));
    }
    for manifest in manifests.into_iter().filter(|m| m.exists()) {
        let patched = read(&manifest)
            .replace(
                &format!("sqlx-sqlite = {{ git = \"{git}\" }}"),
                &format!(
                    "sqlx-sqlite = {{ path = {:?} }}",
                    repo.join("vendor/sqlx-sqlite-stub")
                ),
            )
            .replace(
                &format!("fundamental_build = {{ git = \"{git}\" }}"),
                &format!(
                    "fundamental_build = {{ path = {:?} }}",
                    repo.join("crates/build")
                ),
            )
            .replace(
                &format!("git = \"{git}\""),
                &format!("path = {:?}", repo.display()),
            );
        fs::write(&manifest, patched).unwrap();
    }
}

fn fun_in(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fun"))
        .args(args)
        .env("CARGO_NET_OFFLINE", "true")
        .current_dir(root)
        .output()
        .expect("spawn fun")
}

/// Moves the one domain crate to `domain/shared` as `domain_shared`, the workspace listing members by glob.
fn split_the_domain(root: &Path) {
    let shared = root.join("domain_shared_tmp");
    fs::rename(root.join("domain"), &shared).unwrap();
    fs::create_dir_all(root.join("domain")).unwrap();
    fs::rename(&shared, root.join("domain/shared")).unwrap();
    let manifest = root.join("domain/shared/Cargo.toml");
    let renamed = read(&manifest).replacen("name = \"domain\"", "name = \"domain_shared\"", 1);
    fs::write(&manifest, renamed).unwrap();
    let main = root.join("apps/main/Cargo.toml");
    let repointed = read(&main).replace(
        "domain = { path = \"../../domain\"",
        "domain = { package = \"domain_shared\", path = \"../../domain/shared\"",
    );
    fs::write(&main, repointed).unwrap();
    let workspace = root.join("Cargo.toml");
    let text = read(&workspace);
    let start = text.find("members = [").expect("a members array");
    let end = start + text[start..].find(']').expect("its end") + 1;
    let globbed = format!(
        "{}members = [\"domain/*\", \"apps/*\"]{}",
        &text[..start],
        &text[end..]
    );
    fs::write(&workspace, globbed).unwrap();
}

fn split_domain_project() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("app");
    run_new("app", &root, &["--surfaces", "worker"]);
    split_the_domain(&root);
    (tmp, root)
}

#[test]
fn an_app_added_beside_a_split_domain_builds_a_graph_cargo_resolves() {
    let (_tmp, root) = split_domain_project();
    let members_before = read(&root.join("Cargo.toml"));

    let out = fun_in(
        &root,
        &["add", "app", "billing", "--surfaces", "worker", "--yes"],
    );
    assert!(out.status.success(), "add app failed: {out:?}");

    let app_toml = read(&root.join("apps/billing/Cargo.toml"));
    assert!(
        app_toml.contains(
            "domain = { package = \"domain_shared\", path = \"../../domain/shared\", features = [\"server\"] }"
        ),
        "{app_toml}"
    );
    assert_eq!(
        read(&root.join("Cargo.toml")),
        members_before,
        "`apps/*` already names the new app"
    );
    let git = "https://github.com/sounds-global/fundamental";
    let repo = repo_root();
    let workspace_cargo = root.join("Cargo.toml");
    let local = read(&workspace_cargo)
        .replace(
            &format!("sqlx-sqlite = {{ git = \"{git}\" }}"),
            &format!(
                "sqlx-sqlite = {{ path = {:?} }}",
                repo.join("vendor/sqlx-sqlite-stub")
            ),
        )
        .replace(
            &format!("git = \"{git}\""),
            &format!("path = {:?}", repo.display()),
        );
    fs::write(&workspace_cargo, local).unwrap();
    let metadata = cargo_in_generated(&root, &["metadata", "--format-version", "1"]);
    assert!(
        metadata.status.success(),
        "the workspace does not resolve:\n{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
}

#[test]
fn add_app_asks_which_domain_crate_when_there_are_several() {
    let (_tmp, root) = split_domain_project();
    let second = root.join("domain/billing");
    fs::create_dir_all(&second).unwrap();
    fs::write(
        second.join("Cargo.toml"),
        "[package]\nname = \"domain_billing\"\n",
    )
    .unwrap();

    let out = fun_in(
        &root,
        &["add", "app", "billing", "--surfaces", "worker", "--yes"],
    );
    assert!(
        !out.status.success(),
        "several domain crates must not be guessed between"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CLI_053") && stderr.contains("--domain"),
        "{stderr}"
    );

    let out = fun_in(
        &root,
        &[
            "add",
            "app",
            "billing",
            "--surfaces",
            "worker",
            "--domain",
            "billing",
            "--yes",
        ],
    );
    assert!(!out.status.success(), "a crate lacking `server` is refused");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("CLI_054"), "{stderr}");
    assert!(
        !root.join("apps/billing").exists(),
        "nothing is written when refused"
    );

    let out = fun_in(
        &root,
        &[
            "add",
            "app",
            "billing",
            "--surfaces",
            "worker",
            "--domain",
            "shared",
            "--yes",
        ],
    );
    assert!(out.status.success(), "the named crate is used: {out:?}");
}
