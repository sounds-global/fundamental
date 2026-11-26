//! `fun generate` and `fun seed` discovery, driven through the binary against `fun new` projects.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

mod common;
use common::cargo_in_generated;

fn new_project(name: &str, extra: &[&str]) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join(name);
    let mut args = vec!["new", name];
    args.push(root.to_str().unwrap());
    args.extend_from_slice(extra);
    let status = Command::new(env!("CARGO_BIN_EXE_fun"))
        .args(&args)
        .status()
        .expect("spawn fun new");
    assert!(status.success(), "`fun new` failed: {status:?}");
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

fn assert_cargo_check(root: &Path, args: &[&str], what: &str) {
    let out = cargo_in_generated(root, args);
    assert!(
        out.status.success(),
        "the generated domain must compile for {what}:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The domain `Cargo.toml` declares `engine` forwarding the umbrella's own, listed by `server`.
fn assert_engine_capability(cargo: &str, engine: &str) {
    assert!(
        cargo.contains(&format!("{engine} = [\"fundamental/{engine}\"]")),
        "no `{engine}` capability feature: {cargo}"
    );
    let server = cargo
        .lines()
        .find(|l| l.starts_with("server = "))
        .unwrap_or_else(|| panic!("no `server` feature line: {cargo}"));
    assert!(
        server.contains(&format!("\"{engine}\"")),
        "`server` does not imply `{engine}`: {server}"
    );
}

fn find_with_suffix(dir: &Path, suffix: &str) -> PathBuf {
    let matches: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(suffix))
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected one {suffix} in {dir:?}, got {matches:?}"
    );
    matches[0].clone()
}

#[test]
fn generate_action_writes_dtos_handler_and_reexports() {
    let (_tmp, root) = new_project("acme", &["--yes"]);

    let out = run_in(&root, &["generate", "action", "greet"]);
    assert!(out.status.success(), "generate action failed: {out:?}");

    let action = read(&root.join("domain/src/actions/greet.rs"));
    assert!(action.contains("pub struct GreetRequest {"), "{action}");
    assert!(action.contains("pub struct GreetResponse {"), "{action}");
    assert!(!action.contains("#[cfg"), "{action}");
    assert!(
        action.contains("use fundamental::{action, AnyError};"),
        "{action}"
    );
    assert!(
        action.contains("#[action(POST, \"/api/greet\", reactive)]"),
        "{action}"
    );
    assert!(action.contains("pub async fn greet("), "{action}");

    let actions_mod = read(&root.join("domain/src/actions/mod.rs"));
    assert!(actions_mod.contains("mod greet;"), "{actions_mod}");
    assert!(actions_mod.contains("pub use greet::*;"), "{actions_mod}");
    let lib = read(&root.join("domain/src/lib.rs"));
    assert!(lib.contains("pub mod actions;"), "{lib}");
    assert!(lib.contains("pub use actions::*;"), "{lib}");

    assert!(
        read(&root.join("domain/Cargo.toml")).contains("serde"),
        "serde dep missing"
    );
}

#[test]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn generate_slice_writes_model_resolver_and_feature_matrix() {
    let (_tmp, root) = new_project("acme", &["--yes"]);

    let out = run_in(&root, &["generate", "slice", "mine"]);
    assert!(out.status.success(), "generate slice failed: {out:?}");

    // No database wired, so the example model defaults to the YSQL shape.
    let slice = read(&root.join("domain/src/sync/mine.rs"));
    assert!(
        slice.contains("#[model(offline, syncable, sql, table = \"mine\")]"),
        "{slice}"
    );
    assert!(slice.contains("#[model(owner)]"), "{slice}");
    assert!(slice.contains("#[slice(MineRow, \"mine\")]"), "{slice}");
    assert!(slice.contains("pub struct MineRow"), "{slice}");
    assert!(slice.contains("async fn mine(ctx: SliceContext"), "{slice}");
    assert!(!slice.contains("#[cfg"), "{slice}");

    let sync_mod = read(&root.join("domain/src/sync/mod.rs"));
    assert!(sync_mod.contains("mod mine;"), "{sync_mod}");
    assert!(sync_mod.contains("pub use mine::*;"), "{sync_mod}");
    let lib = read(&root.join("domain/src/lib.rs"));
    assert!(lib.contains("#[cfg(feature = \"sync\")]"), "{lib}");
    assert!(lib.contains("pub mod sync;"), "{lib}");

    let cargo = read(&root.join("domain/Cargo.toml"));
    assert!(cargo.contains("device = ["), "{cargo}");
    assert!(cargo.contains("sync-server = ["), "{cargo}");
    assert!(cargo.contains("fundamental/sync_server"), "{cargo}");
    assert!(cargo.contains("fundamental/ws_client"), "{cargo}");
    assert_engine_capability(&cargo, "sql");
}

#[test]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn generate_model_into_a_domain_only_project() {
    let (_tmp, root) = new_project("acme", &["--yes"]);

    let out = run_in(
        &root,
        &[
            "generate",
            "model",
            "Post",
            "-f",
            "title:String",
            "-f",
            "views:i64",
        ],
    );
    assert!(out.status.success(), "generate model failed: {out:?}");

    let model = read(&root.join("domain/src/models/post.rs"));
    assert!(model.contains("use fundamental::Model;"), "{model}");
    assert!(model.contains("use uuid::Uuid;"), "{model}");
    assert!(
        model.contains(
            "#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, Model)]"
        ),
        "{model}"
    );
    assert!(model.contains("#[model(sql, table = \"post\")]"), "{model}");
    assert!(!model.contains("cfg_attr"), "{model}");
    assert!(model.contains("#[model(id)]"), "{model}");
    assert!(model.contains("pub id: Uuid,"), "{model}");
    assert!(model.contains("pub title: String,"), "{model}");
    assert!(model.contains("pub views: i64,"), "{model}");

    assert!(read(&root.join("domain/src/lib.rs")).contains("pub mod models;"));
    assert!(read(&root.join("domain/src/models/mod.rs")).contains("pub mod post;"));

    let cargo = read(&root.join("domain/Cargo.toml"));
    assert!(cargo.contains("uuid"), "uuid dep missing: {cargo}");
    assert!(cargo.contains("\"v4\""), "uuid v4 feature missing: {cargo}");
    assert!(cargo.contains("serde"), "serde dep missing: {cargo}");
    assert_engine_capability(&cargo, "sql");

    // An empty stub, not DDL derived from the fields.
    let migrations = root.join("domain/migrations/sql");
    let up_body = read(&find_with_suffix(&migrations, "_create_post.up.sql"));
    let down_body = read(&find_with_suffix(&migrations, "_create_post.down.sql"));

    for body in [&up_body, &down_body] {
        assert!(
            body.lines()
                .all(|line| line.trim().is_empty() || line.trim_start().starts_with("--")),
            "a generated migration is a comment-only stub: {body}"
        );
        assert!(!body.contains("post"), "no DDL for the model: {body}");
    }
    assert!(
        up_body.contains("PRIMARY KEY (id HASH)"),
        "the stub teaches a key that declares its sharding: {up_body}"
    );
}

#[test]
fn generate_model_routes_into_the_shared_domain_even_with_a_surface() {
    let (_tmp, root) = new_project("acme", &["--surfaces", "worker"]);

    let out = run_in(&root, &["generate", "model", "Widget", "-f", "sku:String"]);
    assert!(out.status.success(), "generate model failed: {out:?}");

    assert!(root.join("domain/src/models/widget.rs").exists());
    let lib_rs = read(&root.join("domain/src/lib.rs"));
    assert!(lib_rs.contains("pub mod models;"), "{lib_rs}");
    assert!(read(&root.join("domain/src/models/mod.rs")).contains("pub mod widget;"));
    assert!(
        !root.join("apps/main/src/models").exists(),
        "models must route to domain/, not the app"
    );

    let migrations = root.join("domain/migrations/sql");
    let _up = find_with_suffix(&migrations, "_create_widget.up.sql");
    let _down = find_with_suffix(&migrations, "_create_widget.down.sql");
}

#[test]
fn generate_model_targets_cql() {
    let (_tmp, root) = new_project("acme", &["--yes"]);

    let out = run_in(
        &root,
        &[
            "generate",
            "model",
            "Event",
            "-f",
            "payload:String",
            "--db",
            "cql",
        ],
    );
    assert!(out.status.success(), "generate model (cql) failed: {out:?}");

    let model = read(&root.join("domain/src/models/event.rs"));
    assert!(
        model.contains("#[model(cql, table = \"event\")]"),
        "{model}"
    );
    assert!(!model.contains("cfg_attr"), "{model}");
    assert!(model.contains("#[model(partition_key)]"), "{model}");
    let cargo = read(&root.join("domain/Cargo.toml"));
    assert_engine_capability(&cargo, "cql");

    let migrations = root.join("domain/migrations/cql");
    let up = read(&find_with_suffix(&migrations, "_create_event.up.cql"));
    assert!(
        up.lines()
            .all(|line| line.trim().is_empty() || line.trim_start().starts_with("--")),
        "{up}"
    );
    assert!(
        !up.contains("payload"),
        "no DDL derived from the fields: {up}"
    );
}

#[test]
fn generate_model_rejects_a_bad_model_name() {
    let (_tmp, root) = new_project("acme", &["--yes"]);
    let out = run_in(&root, &["generate", "model", "bad-name"]);
    assert!(!out.status.success(), "a bad model name must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CLI_012"),
        "expected InvalidModelName: {stderr}"
    );
}

#[test]
fn generate_model_rejects_a_bad_field_spec() {
    let (_tmp, root) = new_project("acme", &["--yes"]);
    let out = run_in(&root, &["generate", "model", "Post", "-f", "title"]);
    assert!(!out.status.success(), "a bad field spec must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CLI_013"),
        "expected InvalidField: {stderr}"
    );
}

#[test]
fn generate_model_outside_a_project_errors() {
    let tmp = TempDir::new().unwrap();
    let out = run_in(tmp.path(), &["generate", "model", "Post"]);
    assert!(!out.status.success(), "must fail outside a project");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CLI_008"),
        "expected NotInProject: {stderr}"
    );
}

#[test]
fn seed_discovery_targets_the_sole_app_with_seeds() {
    let (_tmp, root) = new_project("acme", &["--surfaces", "worker"]);
    assert!(
        run_in(&root, &["add", "seeds"]).status.success(),
        "add seeds failed"
    );

    // The seeder bin does not exist, so the command fails, but the printed plan shows `-p main`.
    let out = run_in(&root, &["seed"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("-p main"),
        "expected -p main in plan: {stdout}"
    );
}

#[test]
fn seed_discovery_is_ambiguous_with_two_apps() {
    let (_tmp, root) = new_project("acme", &["--surfaces", "worker"]);
    assert!(run_in(&root, &["add", "seeds"]).status.success());
    assert!(
        run_in(&root, &["add", "app", "billing", "--surfaces", "worker"])
            .status
            .success()
    );
    assert!(
        run_in(&root, &["add", "seeds", "--app", "billing"])
            .status
            .success()
    );

    let out = run_in(&root, &["seed"]);
    assert!(!out.status.success(), "ambiguous seeds must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CLI_014"),
        "expected AmbiguousSeedApp: {stderr}"
    );
}

#[test]
fn generated_model_compiles_with_and_without_its_engine() {
    let (_tmp, root) = new_project("proof", &["--yes"]);

    let out = run_in(
        &root,
        &[
            "generate",
            "model",
            "Post",
            "-f",
            "title:String",
            "-f",
            "views:i64",
        ],
    );
    assert!(out.status.success(), "generate model failed: {out:?}");

    point_at_local_repo(&root);

    assert_cargo_check(&root, &["check", "-p", "domain"], "no engine capability");

    assert_cargo_check(
        &root,
        &["check", "-p", "domain", "--features", "sql"],
        "the sql capability",
    );
}

/// Browser bundle, storeless host and server are the three builds a project ships.
#[test]
fn generated_domain_compiles_for_the_browser_a_storeless_host_and_a_server() {
    let (_tmp, root) = new_project("wproof", &["--yes"]);
    assert!(
        run_in(&root, &["generate", "model", "Post", "-f", "title:String"])
            .status
            .success(),
        "generate model failed"
    );
    assert!(
        run_in(&root, &["generate", "action", "greet"])
            .status
            .success(),
        "generate action failed"
    );
    point_at_local_repo(&root);

    assert_cargo_check(
        &root,
        &[
            "check",
            "-p",
            "domain",
            "--target",
            "wasm32-unknown-unknown",
        ],
        "the browser target, types only",
    );
    assert_cargo_check(
        &root,
        &["check", "-p", "domain"],
        "a host that owns no store",
    );
    assert_cargo_check(
        &root,
        &["check", "-p", "domain", "--features", "server"],
        "a server: persistence + handlers",
    );
}

/// Needs the `wasm32-unknown-unknown` target.
#[test]
fn generated_slice_domain_compiles_for_device_and_sync_server() {
    let (_tmp, root) = new_project("sproof", &["--yes"]);
    assert!(
        run_in(&root, &["generate", "slice", "mine"])
            .status
            .success(),
        "generate slice failed"
    );
    point_at_local_repo(&root);

    let device = cargo_in_generated(
        &root,
        &[
            "check",
            "-p",
            "domain",
            "--features",
            "device",
            "--target",
            "wasm32-unknown-unknown",
        ],
    );
    assert!(
        device.status.success(),
        "the generated domain must compile for a wasm device with --features device:\n{}",
        String::from_utf8_lossy(&device.stderr)
    );

    let server = cargo_in_generated(
        &root,
        &["check", "-p", "domain", "--features", "sync-server"],
    );
    assert!(
        server.status.success(),
        "the generated domain must compile with --features sync-server:\n{}",
        String::from_utf8_lossy(&server.stderr)
    );
}

#[test]
fn generated_sync_surface_app_compiles_for_each_backend() {
    for (name, extra) in [
        ("syncapp_cr", &["--surfaces", "sync", "--yes"][..]),
        (
            "syncapp_sc",
            &["--surfaces", "sync", "--db", "cql", "--yes"][..],
        ),
        (
            "syncapp_both",
            &[
                "--surfaces",
                "sync",
                "--db",
                "sql,cql",
                "--sync-store",
                "cql",
                "--yes",
            ][..],
        ),
    ] {
        let (_tmp, root) = new_project(name, extra);
        let main = read(&root.join("apps/main/src/main.rs"));
        assert!(
            main.ends_with("fn main() -> std::process::ExitCode {\n    main::app().run()\n}\n"),
            "{main}"
        );
        let lib = read(&root.join("apps/main/src/lib.rs"));
        assert!(lib.contains(".sync(register_slices)"), "{lib}");
        assert_eq!(
            lib.contains(".sync_store(fundamental::app::SyncStore::Cql)"),
            name == "syncapp_both",
            "{lib}"
        );
        point_at_local_repo(&root);
        let checked = cargo_in_generated(&root, &["check"]);
        assert!(
            checked.status.success(),
            "the generated sync surface app ({name}) failed to compile:\n{}",
            String::from_utf8_lossy(&checked.stderr)
        );
    }
}

/// On Linux the host check runs only when `webkit2gtk-4.1` resolves via pkg-config.
#[test]
fn generated_native_app_compiles() {
    let (_tmp, root) = new_project("nativeproof", &["--surfaces", "web,native,sync", "--yes"]);
    point_at_local_repo(&root);

    let app = root.join("apps/main");
    assert!(app.join("src/live_boot.rs").exists(), "live_boot.rs exists");
    assert!(read(&app.join("src/live_boot.rs")).contains("provide_live_hosted"));
    assert!(read(&app.join("build.rs")).contains("Hydration::Live"));
    assert!(
        read(&app.join("native/src/lib.rs"))
            .contains(".sync_host(sync_host_config(), device_slices())")
    );

    let bundle = cargo_in_generated(
        &root,
        &[
            "check",
            "-p",
            "main",
            "--no-default-features",
            "--features",
            "tauri",
            "--target",
            "wasm32-unknown-unknown",
        ],
    );
    assert!(
        bundle.status.success(),
        "the generated app's webview bundle must compile with --features tauri on wasm:\n{}",
        String::from_utf8_lossy(&bundle.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&bundle.stderr).contains("architecture checks are off"),
        "a new project starts checked:\n{}",
        String::from_utf8_lossy(&bundle.stderr)
    );

    let page = app.join("pages/index.rs");
    let humble = read(&page);
    fs::write(
        &page,
        format!("{humble}\n#[cfg(any())]\nfn planted() {{}}\n"),
    )
    .unwrap();
    let refused = cargo_in_generated(&root, &["check", "-p", "main"]);
    fs::write(&page, humble).unwrap();
    assert!(
        !refused.status.success() && String::from_utf8_lossy(&refused.stderr).contains("BUILD_027"),
        "a planted `cfg` in a page must refuse the generated build:\n{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    if cfg!(target_os = "linux") && !webkit2gtk_available() {
        println!("skipping the native-host compile: webkit2gtk-4.1 not found via pkg-config");
        return;
    }
    let host = cargo_in_generated(&root, &["check", "-p", "main_native"]);
    assert!(
        host.status.success(),
        "the generated native host crate must compile for the desktop target:\n{}",
        String::from_utf8_lossy(&host.stderr)
    );
}

#[test]
fn build_platform_flags_require_the_native_surface() {
    let (_tmp, root) = new_project("nonative", &["--surfaces", "worker", "--yes"]);
    let out = run_in(&root, &["build", "--platform", "ios"]);
    assert!(!out.status.success(), "must refuse without native");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("does not declare a 'native' surface"),
        "expected SurfaceNotDeclared: {stderr}"
    );

    let out = run_in(&root, &["build", "--bundle"]);
    assert!(!out.status.success(), "must refuse --bundle without native");
}

#[test]
fn native_only_app_is_a_library_with_the_native_tree() {
    let (_tmp, root) = new_project("nativeonly", &["--surfaces", "native", "--yes"]);
    let app = root.join("apps/main");

    assert!(!app.join("src/main.rs").exists(), "no server binary");
    let cargo = read(&app.join("Cargo.toml"));
    assert!(!cargo.contains("[[bin]]"), "no [[bin]] section");
    assert!(cargo.contains("tauri = [\"hydrate\", \"fundamental/web-hydrate-tauri\"]"));

    for file in [
        "native/Cargo.toml",
        "native/build.rs",
        "native/tauri.conf.json",
        "native/capabilities/default.json",
        "native/src/lib.rs",
        "native/src/main.rs",
        "native/src/platform_hooks.rs",
        "native/icons/icon.png",
        "native/icons/icon.icns",
        "native/icons/icon.ico",
        "native/icons/32x32.png",
        "native/icons/128x128.png",
        "native/icons/128x128@2x.png",
        "native/platforms/README.md",
        "native/platforms/macos/hooks.rs",
        "native/platforms/windows/hooks.rs",
        "native/platforms/linux/hooks.rs",
        "native/platforms/ios/Package.swift",
        "native/platforms/ios/Sources/HooksPlugin.swift",
        "native/platforms/android/build.gradle.kts",
        "native/platforms/android/src/main/java/HooksPlugin.kt",
        "native/hooks/Cargo.toml",
        "native/hooks/build.rs",
        "native/hooks/src/lib.rs",
    ] {
        assert!(app.join(file).exists(), "missing {file}");
    }

    assert!(common::manifest_text(&root).contains("native = {}"));
    let workspace = read(&root.join("Cargo.toml"));
    assert!(workspace.contains("\"apps/main/native\""));
    assert!(workspace.contains("\"apps/main/native/hooks\""));
    assert!(workspace.contains("tauri-plugin = \"2\""));
    let ignore = read(&root.join(".gitignore"));
    assert!(ignore.contains("apps/*/native/gen/apple/build/"));
    assert!(
        !ignore.contains("apps/*/native/gen/\n"),
        "gen/ itself stays committed"
    );
}

fn webkit2gtk_available() -> bool {
    Command::new("pkg-config")
        .args(["--exists", "webkit2gtk-4.1"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Points `fundamental` and the `sqlx-sqlite` stub at this checkout, for an offline build.
fn point_at_local_repo(root: &Path) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate has a parent (the workspace root)");
    let stub = repo.join("vendor/sqlx-sqlite-stub");
    let git = "https://github.com/sounds-global/fundamental";
    let cargo_path = root.join("Cargo.toml");
    let mut cargo = read(&cargo_path)
        .replace(
            &format!("fundamental = {{ git = \"{git}\" }}"),
            &format!("fundamental = {{ path = \"{}\" }}", repo.display()),
        )
        .replace(
            &format!("sqlx-sqlite = {{ git = \"{git}\" }}"),
            &format!("sqlx-sqlite = {{ path = \"{}\" }}", stub.display()),
        );
    // A db-less project has no `sqlx-sqlite` patch yet, but a YSQL model needs one.
    if !cargo.contains("sqlx-sqlite") {
        cargo.push_str(&format!(
            "\n[patch.crates-io]\nsqlx-sqlite = {{ path = \"{}\" }}\n",
            stub.display()
        ));
    }
    fs::write(&cargo_path, cargo).unwrap();

    let build_crate = repo.join("crates/build");
    for manifest in [
        root.join("apps/main/Cargo.toml"),
        root.join("apps/main/native/Cargo.toml"),
    ] {
        if !manifest.exists() {
            continue;
        }
        let text = read(&manifest).replace(
            &format!("fundamental_build = {{ git = \"{git}\" }}"),
            &format!(
                "fundamental_build = {{ path = \"{}\" }}",
                build_crate.display()
            ),
        );
        fs::write(&manifest, text).unwrap();
    }
}
