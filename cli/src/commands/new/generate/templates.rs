//! File-content templates for the scaffolded project, each a pure function of a [`ProjectPlan`].

use super::readability::{READABILITY_LINTS, READABILITY_THRESHOLDS};

use super::ProjectPlan;

const FUNDAMENTAL_GIT: &str = "https://github.com/sounds-global/fundamental";

/// The generated `main`'s tracing setup: `RUST_LOG`, else `info`; the caller imports `EnvFilter`.
pub(super) const TRACING_INIT: &str = "\x20   tracing_subscriber::fmt()\n\
     \x20       .with_env_filter(\n\
     \x20           EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(\"info\")),\n\
     \x20       )\n\
     \x20       .init();\n";

/// The project's `fun.toml`: its name and version, and the app's table when it has one.
pub(super) fn fun_toml(plan: &ProjectPlan) -> String {
    let mut out = format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", plan.name);
    if let Some(app) = app_manifest(plan) {
        out.push('\n');
        out.push_str(&app);
    }
    out
}

/// The app's table in `fun.toml`, or `None` for a project with no app.
pub(super) fn app_manifest(plan: &ProjectPlan) -> Option<String> {
    if !plan.has_app() {
        return None;
    }
    let app = &plan.app_name;
    let mut out = format!("[apps.{app}]\n");
    if plan.worker {
        out.push_str("worker = {}\n");
    }
    if plan.socket {
        out.push_str(&format!("socket = {{ port = {} }}\n", plan.socket_port));
    }
    if plan.api {
        out.push_str(&format!("api = {{ port = {} }}\n", plan.api_port));
    }
    if plan.sync {
        out.push_str(&format!("sync = {{ port = {} }}\n", plan.sync_port));
    }
    if plan.native {
        out.push_str("native = {}\n");
    }
    if plan.web {
        if plan.tailwind {
            out.push_str(&format!(
                "web = {{ port = {}, css_command = \"tailwindcss -i styles/input.css -o dist/app.css\" }}\n",
                plan.web_port
            ));
        } else {
            out.push_str(&format!("web = {{ port = {} }}\n", plan.web_port));
        }
    }
    for engine in &plan.databases {
        out.push_str(&database_stub(app, &plan.app_crate_name, engine));
    }
    Some(out)
}

/// An `[apps.<app>.database.<engine>]` block of [`Engine::scaffold_config`](crate::engine::Engine::scaffold_config)'s defaults.
fn database_stub(app: &str, db: &str, engine: &crate::engine::Engine) -> String {
    let key = engine.config_key();
    let mut out = format!("\n[apps.{app}.database.{key}]\n");
    for (field, value) in engine.scaffold_config(db) {
        out.push_str(&value.render_toml(field));
    }
    out
}

/// The root workspace `Cargo.toml`, declaring the shared dependencies once.
pub(super) fn workspace_cargo_toml(plan: &ProjectPlan) -> String {
    let mut members = vec!["\"domain\"".to_string()];
    members.extend(
        plan.workspace_members()
            .into_iter()
            .map(|m| format!("\"{m}\"")),
    );
    let members = members.join(", ");
    let mut out = format!(
        "[workspace]\n\
         resolver = \"2\"\n\
         members = [{members}]\n\n\
         [workspace.dependencies]\n\
         fundamental = {{ git = \"{git}\" }}\n\
         tokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"signal\"] }}\n\
         serde = {{ version = \"1\", features = [\"derive\"] }}\n\
         # `log` everywhere, so a crate built alone counts `cognitive_complexity` as the workspace does.\n\
         tracing = {{ version = \"0.1\", features = [\"log\"] }}\n\
         tracing-subscriber = {{ version = \"0.3\", features = [\"env-filter\"] }}\n",
        git = FUNDAMENTAL_GIT,
    );
    if plan.native {
        out.push_str(
            "tauri = \"2\"\n\
             tauri-build = \"2\"\n\
             tauri-plugin = \"2\"\n",
        );
    }
    if uses_sql(plan) {
        out.push_str(&sql_sqlite_patch());
    }
    out.push_str(&readability_lints_toml());
    out.push_str(TEST_PROFILE);
    out
}

/// The workspace's readability lints, each at `warn`.
fn readability_lints_toml() -> String {
    let declared = |clippy: bool| -> String {
        READABILITY_LINTS
            .iter()
            .filter_map(|lint| match lint.strip_prefix("clippy::") {
                Some(name) if clippy => Some(format!("{name} = \"warn\"\n")),
                None if !clippy => Some(format!("{lint} = \"warn\"\n")),
                _ => None,
            })
            .collect()
    };
    let (rust, clippy) = (declared(false), declared(true));
    format!("\n[workspace.lints.rust]\n{rust}\n[workspace.lints.clippy]\n{clippy}")
}

/// The project's `clippy.toml`: each readability lint's bar.
pub(super) fn clippy_toml() -> String {
    let bars: String = READABILITY_THRESHOLDS
        .iter()
        .map(|(key, bar)| format!("{key} = {bar}\n"))
        .collect();
    bars
}

/// The dev profile, which tests inherit: dependencies optimised, the project's own crates not.
const TEST_PROFILE: &str = "\n\
     # Dependencies optimised, the project's crates for debugging; `test` inherits it.\n\
     [profile.dev.package.\"*\"]\n\
     opt-level = 2\n";

/// The project's `.config/nextest.toml`: parallel fast tests, the `agent` profile, and `release`.
pub(super) fn nextest_toml() -> String {
    "# A test that cannot keep to its own rows runs alone, named in an override that\n\
     # `fundamental_test_isolation` reads:\n\
     #\n\
     # [[profile.default.overrides]]\n\
     # filter = 'test(=staff::the_first_admin_is_bootstrapped_once)'\n\
     # threads-required = \"num-test-threads\"\n\
     #\n\
     # Tests in a `stack` or `browser` module run only with `--profile release`.\n\
     [profile.default]\n\
     test-threads = \"num-cpus\"\n\
     default-filter = \"not (test(/^stack::/) or test(/^browser::/))\"\n\
     \n\
     # What `fun test` runs, reporting to `target/nextest/agent/junit.xml`.\n\
     [profile.agent]\n\
     fail-fast = false\n\
     default-filter = \"not (test(/^stack::/) or test(/^browser::/))\"\n\
     \n\
     # Everything: the fast tests, the real stack and the browser.\n\
     [profile.release]\n\
     default-filter = \"all()\"\n\
     fail-fast = false\n\
     \n\
     [profile.agent.junit]\n\
     path = \"junit.xml\"\n"
        .to_string()
}

/// Whether the plan pulls sqlx, and so needs the `sqlx-sqlite` patch; every server surface does.
fn uses_sql(plan: &ProjectPlan) -> bool {
    plan.databases.iter().any(|e| e.feature() != "cql")
        || plan.web
        || plan.worker
        || plan.socket
        || plan.api
        || plan.sync
}

/// sqlx and rusqlite cannot co-resolve `libsqlite3-sys`, and `[patch]` never propagates, so every consumer repeats it.
fn sql_sqlite_patch() -> String {
    format!("\n[patch.crates-io]\nsqlx-sqlite = {{ git = \"{FUNDAMENTAL_GIT}\" }}\n")
}

/// The root `.gitignore`.
pub(super) fn gitignore() -> String {
    // Unconditional, so `fun add surface native` needs no edit; `gen/` itself is committed.
    "/target\n\
     **/*.rs.bk\n\
     /fun.local.toml\n\
     apps/*/native/dist/\n\
     apps/*/native/gen/apple/build/\n\
     apps/*/native/gen/android/.gradle/\n\
     apps/*/native/gen/android/build/\n\
     apps/*/native/gen/android/app/build/\n\
     apps/*/native/platforms/.tauri/\n"
        .to_string()
}

/// The root `README.md`.
pub(super) fn readme(plan: &ProjectPlan) -> String {
    format!(
        "# {name}\n\n\
         Built with [Fundamental](https://github.com/sounds-global/fundamental).\n\n\
         ## Structure\n\n\
         The `domain/` crate holds your shared model types; every app depends on it. \
         Run `fun generate model <Name>` to add a model there. Each surface lives in \
         its own `apps/<name>/` crate.\n\n\
         ## Building\n\n\
         This project depends on `fundamental` straight from its git repository, declared \
         once in the workspace `Cargo.toml` under `[workspace.dependencies]`. To build \
         against a local checkout instead, change that `git` line to a `path`.\n",
        name = plan.name,
    )
}

/// The shared `domain` library crate's `Cargo.toml`: each engine a feature `server` implies.
pub(super) fn domain_cargo_toml(plan: &ProjectPlan) -> String {
    // `#[derive(Model)]` emits an engine's impls under `#[cfg(feature = "<engine>")]`, so the names are fixed.
    let mut engine_features = String::new();
    let mut server_engines = String::new();
    for engine in &plan.databases {
        let feature = match engine.feature() {
            "cql" => "cql",
            _ => "sql",
        };
        engine_features.push_str(&format!("{feature} = [\"fundamental/{feature}\"]\n"));
        server_engines.push_str(&format!(", \"{feature}\""));
    }
    format!(
        "[package]\n\
         name = \"domain\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\n\
         [lints]\n\
         workspace = true\n\n\
         [dependencies]\n\
         fundamental = {{ workspace = true }}\n\
         serde = {{ workspace = true }}\n\n\
         # `#[action]` gates its route off the wasm target, so the action layer is native-only.\n\
         [target.'cfg(not(target_arch = \"wasm32\"))'.dependencies]\n\
         fundamental = {{ workspace = true, features = [\"server\"] }}\n\n\
         [features]\n\
         default = []\n\n\
         # \"This build is a server\": it implies the engines its handlers call.\n\
         server = [\"fundamental/server\"{server_engines}]\n\n\
         {engine_features}\n\
         # Slice sync: a browser or device build enables `device`, a slice server `sync-server`.\n\
         sync = [\"fundamental/sync\"]\n\
         device = [\"sync\", \"fundamental/ws_client\"]\n\
         sync-server = [\"sync\", \"fundamental/sync_server\"]\n"
    )
}

/// The shared `domain` library crate's `src/lib.rs`, declaring the modules the generators write into.
pub(super) fn domain_lib_rs(plan: &ProjectPlan) -> String {
    format!(
        "//! Domain types and logic for the `{name}` project, compiled into every surface.\n\n\
         pub mod actions;\n\
         pub mod models;\n\
         #[cfg(feature = \"sync\")]\n\
         pub mod sync;\n",
        name = plan.name
    )
}

/// The `domain/src/sync/mod.rs` stub, compiled only under the `sync` feature.
pub(super) fn domain_sync_mod_rs() -> String {
    "//! Syncable models and their `#[slice]` resolvers.\n".to_string()
}

/// The `domain/src/models/mod.rs` stub: the home for `#[derive(Model)]` types.
pub(super) fn domain_models_mod_rs() -> String {
    "//! Domain models (`#[derive(Model)]`).\n".to_string()
}

/// The `domain/src/actions/mod.rs` stub: the home for `#[action]` handlers and their types.
pub(super) fn domain_actions_mod_rs() -> String {
    "//! Domain actions (`#[action]`) and their request/response types.\n".to_string()
}

/// Whether the slice store is YCQL-backed: `sync_store` if chosen, else YSQL when present.
pub(super) fn sync_uses_cql(plan: &ProjectPlan) -> bool {
    use crate::engine::Engine;
    match &plan.sync_store {
        Some(Engine::CqlDb) => true,
        Some(_) => false,
        None => {
            let has_sql = plan
                .databases
                .iter()
                .any(|e| matches!(e, Engine::SqlDb | Engine::Other(_)));
            !has_sql && plan.databases.contains(&Engine::CqlDb)
        }
    }
}

/// The web app crate's `Cargo.toml`: an `rlib` beside the server binary, with `ssr` and `hydrate` modes.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(super) fn web_app_cargo_toml(plan: &ProjectPlan) -> String {
    // A native-only crate's ssr build serves the sibling `native/` crate, which needs only `web`.
    let mut ssr = vec!["\"fundamental/web\""];
    if plan.web {
        ssr.push("\"fundamental/app\"");
    }
    ssr.extend(super::app_templates::web_surface_features(plan));
    if plan.sync {
        ssr.push(if sync_uses_cql(plan) {
            "\"fundamental/cql\""
        } else {
            "\"fundamental/sql\""
        });
        ssr.push("\"domain/sync-server\"");
    }
    ssr.push("\"domain/server\"");
    ssr.push("\"leptos/ssr\"");
    if plan.web {
        ssr.extend([
            "\"dep:tokio\"",
            "\"dep:tracing\"",
            "\"dep:tracing-subscriber\"",
        ]);
    }
    let ssr = ssr.join(", ");
    // A sync app's browser build is the device: offline mirror, sync engine and `use_live`.
    let hydrate = if plan.sync {
        "[\"fundamental/web-hydrate\", \"fundamental/web-live\", \"fundamental/sync\", \"fundamental/ws_client\", \"domain/device\", \"leptos/hydrate\"]"
    } else {
        "[\"fundamental/web-hydrate\", \"leptos/hydrate\"]"
    };
    // A native-only app crate is a library hosted by the sibling `native/` crate.
    let bin_section = if plan.web {
        format!(
            "[[bin]]\n\
             name = \"{crate_name}\"\n\
             path = \"src/main.rs\"\n\
             required-features = [\"ssr\"]\n\n",
            crate_name = plan.app_crate_name
        )
    } else {
        String::new()
    };
    let domain = plan.domain.dependency(2, &[]);
    let bin_deps = if plan.web {
        "tokio = { workspace = true, optional = true }\n\
         tracing = { workspace = true, optional = true }\n\
         tracing-subscriber = { workspace = true, optional = true }\n"
    } else {
        ""
    };
    // The hydrate build with actions sent over Tauri invoke instead of fetch.
    let tauri_feature = if plan.native {
        "tauri = [\"hydrate\", \"fundamental/web-hydrate-tauri\"]\n"
    } else {
        ""
    };
    format!(
        "[package]\n\
         name = \"{crate_name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\n\
         [lints]\n\
         workspace = true\n\n\
         {bin_section}\
         [dependencies]\n\
         fundamental = {{ workspace = true }}\n\
         {domain}\n\
         leptos = {{ version = \"0.8\", default-features = false, features = [\"islands\"] }}\n\
         serde = {{ workspace = true }}\n\
         wasm-bindgen = \"0.2\"\n\
         {bin_deps}\n\
         [build-dependencies]\n\
         fundamental_build = {{ git = \"{git}\" }}\n\n\
         [features]\n\
         default = [\"ssr\"]\n\
         ssr = [{ssr}]\n\
         hydrate = {hydrate}\n\
         # Enabled by `fun dev`, never by `fun build`.\n\
         introspect = [\"fundamental/introspect\"]\n\
         {tauri_feature}",
        crate_name = plan.app_crate_name,
        git = FUNDAMENTAL_GIT,
    )
}

/// The web app's `src/lib.rs`: the hydrate entry, the action mounts, and the server's `App`.
pub(super) fn web_lib_rs(plan: &ProjectPlan) -> String {
    let app = &plan.app_name;
    let server = if plan.web {
        let gate = super::app_templates::ssr_gate();
        format!(
            "\n{}{}",
            super::app_templates::surface_items(plan, gate),
            super::app_templates::app_fn(plan, gate)
        )
    } else {
        String::new()
    };
    let slices = if plan.sync {
        "\n// Called by the hydrate entry before any island hydrates.\n\
         #[cfg(feature = \"hydrate\")]\n\
         mod live_boot;\n\
         #[cfg(feature = \"hydrate\")]\n\
         pub(crate) use live_boot::live_boot;\n"
    } else {
        ""
    };
    format!(
        "//! `{app}` app: browser (wasm) entry, built with `--features hydrate`.\n\n\
         #[cfg(feature = \"hydrate\")]\n\
         include!(concat!(env!(\"OUT_DIR\"), \"/hydrate.rs\"));\n\n\
         // List each `domain::<Action>` this app serves; an unlisted one has no stub to call.\n\
         fundamental::mount_actions! {{}}\n\
         {slices}{server}"
    )
}

/// The web app's `build.rs`: one `PagesBuild` call over `pages/` (`Hydration::Live` on a sync app).
pub(super) fn web_build_rs(plan: &ProjectPlan) -> String {
    let boot = if plan.sync { "Live" } else { "Plain" };
    format!(
        "//! Generates the page routes, the hydrate entry and the address table from `pages/`.\n\n\
         fn main() -> std::process::ExitCode {{\n\
         \x20   fundamental_build::PagesBuild::run_build_script(fundamental_build::Hydration::{boot})\n\
         }}\n"
    )
}

/// The web app's `src/live_boot.rs`: the live-data bootstrap run before any island hydrates.
pub(super) fn web_live_boot_rs(plan: &ProjectPlan) -> String {
    let app = &plan.app_name;
    if plan.native {
        format!(
            "//! Boots the live-data context before any island hydrates.\n\n\
             /// Provides the app-wide live context: the host's database in the native webview.\n\
             pub(crate) fn live_boot() {{\n\
             \x20   #[cfg(feature = \"tauri\")]\n\
             \x20   {{\n\
             \x20       use fundamental::web::live::provide_live_hosted;\n\
             \x20       provide_live_hosted();\n\
             \x20   }}\n\
             \x20   // A browser deployment provides its own, with a session token:\n\
             \x20   #[cfg(not(feature = \"tauri\"))]\n\
             \x20   {{\n\
             \x20       // use fundamental::core::secrets::SecretString;\n\
             \x20       // use fundamental::offline::db::OfflineConfig;\n\
             \x20       // use fundamental::web::live::provide_live_lazy;\n\
             \x20       // use fundamental::ws_client::SyncClientConfig;\n\
             \x20       // let config = SyncClientConfig::new(\"wss://sync.example.com\", token);\n\
             \x20       // provide_live_lazy(config, OfflineConfig::persistent(\"{app}\"));\n\
             \x20   }}\n\
             }}\n"
        )
    } else {
        format!(
            "//! Boots the live-data context before any island hydrates.\n\n\
             /// Provides the app-wide live context; until it is wired, live views render empty.\n\
             pub(crate) fn live_boot() {{\n\
             \x20   // use fundamental::core::secrets::SecretString;\n\
             \x20   // use fundamental::offline::db::OfflineConfig;\n\
             \x20   // use fundamental::web::live::provide_live_lazy;\n\
             \x20   // use fundamental::ws_client::SyncClientConfig;\n\
             \x20   //\n\
             \x20   // let token = SecretString::new(/* your session token */.into());\n\
             \x20   // let config = SyncClientConfig::new(\"ws://localhost:{sync_port}\", token);\n\
             \x20   // provide_live_lazy(config, OfflineConfig::persistent(\"{app}\"));\n\
             }}\n",
            sync_port = plan.sync_port,
        )
    }
}

/// The sample home page (`pages/index.rs`), served at `/`.
pub(super) fn web_page_index_rs(islands_crate: bool) -> String {
    let place = if islands_crate {
        "// Write an `#[island]` in `apps/<app>/islands` and render it from `page`; never in `pages/`.\n"
    } else {
        "// Write an `#[island]` component and render it from `page`, never on `page` itself.\n"
    };
    format!(
        "//! The home page, served at `/`.\n\n\
         use fundamental::web::prelude::*;\n\n\
         /// The page rendered at `/`. Add more files under `pages/` for more routes.\n\
         pub fn page() -> impl IntoView {{\n\
         \x20   view! {{ <main><h1>\"Welcome to Fundamental\"</h1></main> }}\n\
         }}\n\n\
         {place}\
         //\n\
         // #[island]\n\
         // fn Counter() -> impl IntoView {{\n\
         //     let (count, set) = signal(0);\n\
         //     view! {{\n\
         //         <button on:click=move |_| set.update(|n| *n += 1)>\n\
         //             \"clicks: \" {{move || count.get()}}\n\
         //         </button>\n\
         //     }}\n\
         // }}\n\
         //\n\
         // Then in `page`: view! {{ <main><Counter/></main> }}\n"
    )
}

/// The Tailwind (v4) entry stylesheet, scanning `pages/` for class names.
pub(super) fn tailwind_input_css() -> String {
    "@import \"tailwindcss\";\n@source \"../pages\";\n".to_string()
}

/// `pages/document.rs`, the document shell; `None` when the site's `App::document` is the shell.
pub(super) fn web_document_rs(plan: &ProjectPlan) -> Option<String> {
    if plan.web {
        return None;
    }
    let (link, hint) = if plan.tailwind {
        (
            "<link rel=\\\"stylesheet\\\" href=\\\"/_fundamental/app.css\\\">",
            String::new(),
        )
    } else {
        (
            "",
            "         \x20   // A stylesheet built into dist/app.css (by a `css_command`) is linked with:\n\
             \x20   //   <link rel=\"stylesheet\" href=\"/_fundamental/app.css\">\n"
                .to_string(),
        )
    };
    Some(format!(
        "//! The HTML document shell wrapping every rendered page.\n\n\
         use fundamental::web::PageContext;\n\n\
         /// Wraps the rendered `body` in the HTML document that loads the wasm bundle.\n\
         pub fn document(ctx: &PageContext, body: &str) -> String {{\n\
         \x20   let boot = ctx.hydration_script(\"/_fundamental/{crate_name}.js\");\n\
         {hint}\
         \x20   // `en` until the surface configures a `LocalePolicy`.\n\
         \x20   let lang = ctx.locale().map_or(\"en\", |locale| locale.language());\n\
         \x20   let head = \"<meta charset=\\\"utf-8\\\"><meta name=\\\"viewport\\\" content=\\\"width=device-width, initial-scale=1\\\">{link}<title>{app}</title>\";\n\
         \x20   format!(\"<!DOCTYPE html><html lang=\\\"{{lang}}\\\"><head>{{head}}{{boot}}</head><body>{{body}}</body></html>\")\n\
         }}\n",
        crate_name = plan.app_crate_name,
        app = plan.app_name,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::app_templates::{app_cargo_toml, app_lib_rs, app_main_rs};
    use super::*;
    use crate::Surface;
    use std::path::PathBuf;

    fn plan(surfaces: &[Surface]) -> ProjectPlan {
        ProjectPlan::new("demo", PathBuf::from("demo"), surfaces)
    }

    fn combinations(web: bool) -> Vec<Vec<Surface>> {
        let mut all = Vec::new();
        for mask in 1_u8..16 {
            let mut surfaces: Vec<Surface> = [
                Surface::Worker,
                Surface::Socket,
                Surface::Api,
                Surface::Sync,
            ]
            .into_iter()
            .enumerate()
            .filter(|(bit, _)| mask & (1 << bit) != 0)
            .map(|(_, surface)| surface)
            .collect();
            if web {
                surfaces.push(Surface::Web);
            }
            all.push(surfaces);
        }
        if web {
            all.push(vec![Surface::Web]);
        }
        all
    }

    fn parses(name: &str, src: &str) {
        if let Err(e) = syn::parse_file(src) {
            panic!("generated {name} is invalid: {e}\n{src}");
        }
    }

    #[test]
    fn each_surface_combination_parses() {
        for surfaces in combinations(false) {
            let plan = plan(&surfaces);
            parses(&format!("lib.rs for {surfaces:?}"), &app_lib_rs(&plan));
            parses(&format!("main.rs for {surfaces:?}"), &app_main_rs(&plan));
        }
        for surfaces in combinations(true) {
            parses(
                &format!("web lib.rs for {surfaces:?}"),
                &web_lib_rs(&plan(&surfaces)),
            );
        }
    }

    #[test]
    fn every_main_runs_the_app_in_one_line() {
        let one_line = "fn main() -> std::process::ExitCode {\n    main::app().run()\n}\n";
        for surfaces in combinations(false).into_iter().chain(combinations(true)) {
            let main = app_main_rs(&plan(&surfaces));
            assert!(main.ends_with(one_line), "{surfaces:?}: {main}");
            assert!(!main.contains("Runtime"), "{main}");
            assert!(!main.contains("tokio::main"), "{main}");
        }
    }

    #[test]
    fn the_app_declares_one_line_per_surface() {
        let lib = app_lib_rs(&plan(&[
            Surface::Worker,
            Surface::Socket,
            Surface::Api,
            Surface::Sync,
        ]));
        assert!(
            lib.contains("pub fn app() -> fundamental::App<()> {"),
            "{lib}"
        );
        assert!(lib.contains("    fundamental::app!()\n"), "{lib}");
        for line in [
            ".api(register_api)",
            ".actions(describe_actions)",
            ".worker(register_workers)",
            ".messages(describe_messages)",
            ".socket(|router| router.route(on_ping_route()))",
            ".socket_config(fundamental::ws::WsConfig::public)",
            ".sync(register_slices)",
            ".projections(register_projections)",
            ".state(|_| Ok(()))",
        ] {
            assert!(lib.contains(line), "{line}: {lib}");
        }
        assert!(
            lib.contains("fundamental::mount_workers! { on_hello }"),
            "{lib}"
        );
        assert!(lib.contains("fundamental::mount_slices! {}"), "{lib}");
        assert!(!lib.contains("Runtime"), "{lib}");
        assert!(!lib.contains("surface_enabled"), "{lib}");

        let worker = app_lib_rs(&plan(&[Surface::Worker]));
        assert!(worker.contains(".worker(register_workers)"), "{worker}");
        assert!(
            !worker.contains(".state("),
            "a worker serves no state: {worker}"
        );
        assert!(!worker.contains(".api("), "{worker}");
    }

    #[test]
    fn an_api_app_mounts_its_actions_and_declares_the_dev_tools() {
        let api = plan(&[Surface::Api]);
        let lib = app_lib_rs(&api);
        assert!(lib.contains("fundamental::mount_actions! {}"), "{lib}");
        assert!(lib.contains("Ok(register_actions(router)?.route("), "{lib}");
        let toml = app_cargo_toml(&api);
        assert!(toml.contains("features = [\"app\", \"api\"]"), "{toml}");
        assert!(
            toml.contains("introspect = [\"fundamental/introspect\"]"),
            "{toml}"
        );
        let manifest = app_manifest(&api).unwrap_or_default();
        assert!(manifest.contains("api = { port = 16001 }"), "{manifest}");
    }

    #[test]
    fn a_sync_app_asks_for_the_sync_server_and_its_store() {
        use crate::engine::Engine;

        let sql = plan(&[Surface::Sync]).with_databases(vec![Engine::SqlDb]);
        let toml = app_cargo_toml(&sql);
        assert!(toml.contains("\"sync_server\""), "{toml}");
        assert!(toml.contains("\"sql\""), "{toml}");
        let manifest = app_manifest(&sql).unwrap_or_default();
        assert!(manifest.contains("sync = { port = 9100 }"), "{manifest}");

        let cql = plan(&[Surface::Sync]).with_databases(vec![Engine::CqlDb]);
        assert!(app_cargo_toml(&cql).contains("\"cql\""));
        assert!(app_lib_rs(&cql).contains("default CQL keyspace"));
        assert!(!app_lib_rs(&cql).contains(".sync_store("));
    }

    #[test]
    fn a_sync_app_told_cql_beside_sql_names_its_store() {
        use crate::engine::Engine;

        let both = plan(&[Surface::Sync]).with_databases(vec![Engine::SqlDb, Engine::CqlDb]);
        assert!(!app_lib_rs(&both).contains(".sync_store("));
        let over_cql = both.with_sync_store(Some(Engine::CqlDb));
        let lib = app_lib_rs(&over_cql);
        assert!(
            lib.contains("        .sync_store(fundamental::app::SyncStore::Cql)\n"),
            "{lib}"
        );
    }

    #[test]
    fn subscribe_is_aliased_only_when_both_surfaces_are_present() {
        let both = app_lib_rs(&plan(&[Surface::Worker, Surface::Socket]));
        assert!(both.contains("subscribe as nats_subscribe"), "{both}");
        assert!(both.contains("subscribe as ws_subscribe"), "{both}");
        assert!(both.contains("#[nats_subscribe]"), "{both}");
        assert!(
            both.contains("#[ws_subscribe(socket = \"ping\")]"),
            "{both}"
        );

        let worker = app_lib_rs(&plan(&[Surface::Worker]));
        assert!(!worker.contains(" as nats_subscribe"), "{worker}");
        assert!(worker.contains("#[subscribe]"), "{worker}");
    }

    #[test]
    fn a_web_app_declares_every_surface_in_its_library() {
        let web = plan(&[Surface::Web, Surface::Worker, Surface::Api]);
        let lib = web_lib_rs(&web);
        assert!(
            lib.contains("#[cfg(feature = \"ssr\")]\npub fn app() -> fundamental::App<()> {"),
            "{lib}"
        );
        assert!(
            lib.contains(".pages(|router| register_actions(register_pages(router)?))"),
            "{lib}"
        );
        assert!(lib.contains(".worker(register_workers)"), "{lib}");
        assert!(lib.contains(".api(register_api)"), "{lib}");
        assert!(lib.contains("Ok(router.route(Route::post("), "{lib}");
        assert!(
            lib.contains("#[cfg(feature = \"ssr\")]\nfundamental::mount_workers! { on_hello }"),
            "{lib}"
        );
        assert_eq!(
            lib.matches("fundamental::mount_actions!").count(),
            1,
            "{lib}"
        );
        assert!(web_document_rs(&web).is_none());
    }

    #[test]
    fn a_web_apps_server_build_carries_every_surface() {
        let web = plan(&[Surface::Web, Surface::Worker, Surface::Api]);
        let toml = web_app_cargo_toml(&web);
        assert!(toml.contains("\"fundamental/app\""), "{toml}");
        assert!(toml.contains("\"fundamental/nats\""), "{toml}");
        assert!(toml.contains("\"fundamental/api\""), "{toml}");
        assert!(!toml.contains("\"fundamental/runtime\""), "{toml}");
        assert!(toml.contains("required-features = [\"ssr\"]"), "{toml}");
        let hydrate = toml
            .lines()
            .find(|l| l.trim_start().starts_with("hydrate ="))
            .expect("hydrate feature line");
        assert!(
            !hydrate.contains("nats") && !hydrate.contains("/api"),
            "{hydrate}"
        );

        let manifest = app_manifest(&web).unwrap_or_default();
        assert!(manifest.contains("worker = {}"), "{manifest}");
        assert!(manifest.contains("web = { port = 11001 }"), "{manifest}");
    }

    #[test]
    fn a_native_only_crate_keeps_its_document_and_declares_no_app() {
        let native = plan(&[Surface::Native]);
        assert!(web_document_rs(&native).is_some());
        assert!(!web_lib_rs(&native).contains("pub fn app()"));
    }

    #[test]
    fn web_app_sources_parse() {
        let plan = plan(&[Surface::Web]);
        for (name, src) in [
            ("main.rs", app_main_rs(&plan)),
            ("lib.rs", web_lib_rs(&plan)),
            ("build.rs", web_build_rs(&plan)),
            ("pages/index.rs", web_page_index_rs(true)),
            ("pages/index.rs", web_page_index_rs(false)),
        ] {
            syn::parse_file(&src)
                .unwrap_or_else(|e| panic!("generated web {name} is invalid: {e}\n{src}"));
        }
    }

    #[test]
    fn sync_app_wires_the_live_bootstrap() {
        let websync = plan(&[Surface::Web, Surface::Sync]);
        let build = web_build_rs(&websync);
        assert!(
            build.contains("PagesBuild::run_build_script(fundamental_build::Hydration::Live)"),
            "{build}"
        );
        let lib = web_lib_rs(&websync);
        assert!(lib.contains("mod live_boot;"), "{lib}");
        assert!(
            lib.contains("pub(crate) use live_boot::live_boot;"),
            "{lib}"
        );
        assert!(lib.contains(".sync(register_slices)"), "{lib}");
        let toml = web_app_cargo_toml(&websync);
        assert!(toml.contains("\"fundamental/web-live\""), "{toml}");
        assert!(toml.contains("\"fundamental/sync_server\""), "{toml}");

        let web = plan(&[Surface::Web]);
        assert!(
            web_build_rs(&web)
                .contains("PagesBuild::run_build_script(fundamental_build::Hydration::Plain)"),
            "{}",
            web_build_rs(&web)
        );
        assert!(!web_lib_rs(&web).contains("live_boot"));
        assert!(!web_app_cargo_toml(&web).contains("web-live"));
    }

    #[test]
    fn live_boot_variants_parse_and_match_their_surfaces() {
        let native_sync = plan(&[Surface::Web, Surface::Native, Surface::Sync]);
        let src = web_live_boot_rs(&native_sync);
        syn::parse_file(&src).unwrap_or_else(|e| panic!("{e}:\n{src}"));
        assert!(src.contains("provide_live_hosted();"), "{src}");
        assert!(src.contains("#[cfg(feature = \"tauri\")]"), "{src}");

        let web_sync = plan(&[Surface::Web, Surface::Sync]);
        let src = web_live_boot_rs(&web_sync);
        syn::parse_file(&src).unwrap_or_else(|e| panic!("{e}:\n{src}"));
        assert!(!src.contains("host_sync_config"), "{src}");
        assert!(src.contains("your session token"), "{src}");
        assert!(src.contains("ws://localhost:9100"), "{src}");
    }

    #[test]
    fn web_app_cargo_and_manifest_are_wired() {
        let plan = plan(&[Surface::Web]);
        let toml = web_app_cargo_toml(&plan);
        assert!(!toml.contains("cdylib"), "{toml}");
        assert!(
            toml.contains("ssr = [\"fundamental/web\", \"fundamental/app\""),
            "{toml}"
        );
        assert!(
            toml.contains("hydrate = [\"fundamental/web-hydrate\""),
            "{toml}"
        );
        assert!(toml.contains("fundamental_build = { git ="), "{toml}");

        let manifest = app_manifest(&plan).unwrap_or_default();
        assert!(manifest.contains("web = { port = 11001 }"), "{manifest}");
    }

    #[test]
    fn tailwind_wires_the_css_command_link_and_input() {
        let plan = plan(&[Surface::Web, Surface::Worker]).with_tailwind(true);

        let manifest = app_manifest(&plan).unwrap_or_default();
        assert!(
            manifest.contains("css_command = \"tailwindcss -i styles/input.css -o dist/app.css\""),
            "{manifest}"
        );
        let lib = web_lib_rs(&plan);
        assert!(lib.contains(".head(style_links)"), "{lib}");
        assert!(lib.contains("/_fundamental/app.css"), "{lib}");

        let input = tailwind_input_css();
        assert!(input.contains("@import \"tailwindcss\""), "{input}");
        assert!(input.contains("@source \"../pages\""), "{input}");

        let plain_plan = plan.with_tailwind(false);
        assert!(
            !app_manifest(&plain_plan)
                .unwrap_or_default()
                .contains("css_command")
        );
        assert!(!web_lib_rs(&plain_plan).contains("style_links"));
    }

    #[test]
    fn sql_plan_emits_the_sqlx_sqlite_patch() {
        use crate::engine::Engine;

        let workspace =
            workspace_cargo_toml(&plan(&[Surface::Web]).with_databases(vec![Engine::SqlDb]));
        assert!(workspace.contains("[patch.crates-io]"), "{workspace}");
        assert!(
            workspace.contains(&format!("sqlx-sqlite = {{ git = \"{FUNDAMENTAL_GIT}\" }}")),
            "{workspace}"
        );
    }

    #[test]
    fn every_app_with_a_server_surface_emits_the_sqlx_sqlite_patch() {
        use crate::engine::Engine;

        let workspace =
            workspace_cargo_toml(&plan(&[Surface::Api]).with_databases(vec![Engine::CqlDb]));
        assert!(workspace.contains("sqlx-sqlite"), "{workspace}");
        assert!(workspace_cargo_toml(&plan(&[Surface::Worker])).contains("[patch.crates-io]"));
    }

    #[test]
    fn an_appless_plan_omits_the_sqlx_sqlite_patch() {
        let workspace = workspace_cargo_toml(&plan(&[]));
        assert!(!workspace.contains("[patch.crates-io]"), "{workspace}");
    }

    #[test]
    fn app_cargo_enables_the_app() {
        let toml = app_cargo_toml(&plan(&[Surface::Worker]));
        assert!(toml.contains("features = [\"app\", \"nats\"]"), "{toml}");
    }

    #[test]
    fn the_test_profile_optimises_dependencies() {
        for layout in [vec![Surface::Worker], vec![]] {
            let workspace = workspace_cargo_toml(&plan(&layout));
            let parsed: toml::Table = workspace.parse().expect("valid TOML");
            assert!(parsed["profile"].get("test").is_none(), "{workspace}");
            assert_eq!(
                parsed["profile"]["dev"]["package"]["*"]["opt-level"].as_integer(),
                Some(2),
                "{workspace}"
            );
        }
    }

    #[test]
    fn the_nextest_configuration_runs_tests_in_parallel() {
        let parsed: toml::Table = nextest_toml().parse().expect("valid TOML");
        assert_eq!(
            parsed["profile"]["default"]["test-threads"].as_str(),
            Some("num-cpus")
        );
        assert!(parsed["profile"]["default"].get("overrides").is_none());
        assert_eq!(
            parsed["profile"]["agent"]["fail-fast"].as_bool(),
            Some(false)
        );
        assert_eq!(
            parsed["profile"]["agent"]["junit"]["path"].as_str(),
            Some("junit.xml")
        );
    }

    #[test]
    fn the_slow_tests_wait_for_a_release() {
        let parsed: toml::Table = nextest_toml().parse().expect("valid TOML");
        let fast = "not (test(/^stack::/) or test(/^browser::/))";
        assert_eq!(
            parsed["profile"]["default"]["default-filter"].as_str(),
            Some(fast)
        );
        assert_eq!(
            parsed["profile"]["agent"]["default-filter"].as_str(),
            Some(fast)
        );
        assert_eq!(
            parsed["profile"]["release"]["default-filter"].as_str(),
            Some("all()")
        );
    }

    #[test]
    fn fundamental_is_a_workspace_git_dependency() {
        for layout in [vec![Surface::Worker], vec![]] {
            let workspace = workspace_cargo_toml(&plan(&layout));
            assert!(
                workspace.contains(&format!("fundamental = {{ git = \"{FUNDAMENTAL_GIT}\" }}")),
                "{workspace}"
            );
            assert!(
                workspace.contains("[workspace.dependencies]"),
                "{workspace}"
            );
        }

        let app = app_cargo_toml(&plan(&[Surface::Worker]));
        let domain = domain_cargo_toml(&plan(&[]));
        assert!(
            app.contains("fundamental = { workspace = true, features = ["),
            "{app}"
        );
        assert!(app.contains("serde = { workspace = true }"), "{app}");
        assert!(
            domain.contains("fundamental = { workspace = true }"),
            "{domain}"
        );
        for toml in [&app, &domain] {
            assert!(!toml.contains("git ="), "{toml}");
        }
    }

    #[test]
    fn workspace_members_pair_the_domain_with_each_app() {
        let app = workspace_cargo_toml(&plan(&[Surface::Worker]));
        assert!(
            app.contains("members = [\"domain\", \"apps/main\"]"),
            "{app}"
        );

        let library = workspace_cargo_toml(&plan(&[]));
        assert!(library.contains("members = [\"domain\"]"), "{library}");
    }

    #[test]
    fn app_asks_the_shared_domain_crate_for_its_server_capability() {
        let toml = app_cargo_toml(&plan(&[Surface::Worker]));
        assert!(
            toml.contains("domain = { path = \"../../domain\", features = [\"server\"] }"),
            "{toml}"
        );

        let domain = domain_cargo_toml(&plan(&[]));
        assert!(domain.contains("name = \"domain\""), "{domain}");
        assert!(
            domain.contains("server = [\"fundamental/server\"]"),
            "{domain}"
        );
        assert!(domain.contains("default = []"), "{domain}");
    }

    #[test]
    fn a_chosen_engine_becomes_a_capability_feature_that_server_implies() {
        let domain = domain_cargo_toml(
            &plan(&[Surface::Api]).with_databases(vec![crate::engine::Engine::SqlDb]),
        );
        assert!(domain.contains("sql = [\"fundamental/sql\"]"), "{domain}");
        let server = domain
            .lines()
            .find(|l| l.starts_with("server = "))
            .expect("a server feature line");
        assert!(server.contains("\"fundamental/server\""), "{server}");
        assert!(server.contains("\"sql\""), "{server}");
    }

    #[test]
    fn only_the_chosen_engine_gets_a_capability_feature() {
        let domain = domain_cargo_toml(
            &plan(&[Surface::Api]).with_databases(vec![crate::engine::Engine::CqlDb]),
        );
        assert!(domain.contains("cql = [\"fundamental/cql\"]"), "{domain}");
        assert!(!domain.contains("sql"), "{domain}");
    }

    #[test]
    fn a_database_less_project_links_no_driver_on_any_surface() {
        let domain = domain_cargo_toml(&plan(&[Surface::Native]));
        assert!(
            domain.contains("server = [\"fundamental/server\"]"),
            "{domain}"
        );
        assert!(!domain.contains("sql"), "{domain}");
        assert!(!domain.contains("cql"), "{domain}");
    }

    #[test]
    fn the_domain_lib_gates_neither_the_models_nor_the_action_types() {
        let lib = domain_lib_rs(&plan(&[Surface::Api]));
        assert!(lib.contains("pub mod models;"), "{lib}");
        assert!(lib.contains("pub mod actions;"), "{lib}");
        assert!(!lib.contains("target_arch"), "{lib}");
        assert_eq!(lib.matches("#[cfg(feature").count(), 1, "{lib}");
        assert!(lib.contains("#[cfg(feature = \"sync\")]"), "{lib}");
    }

    #[test]
    fn only_the_web_apps_ssr_build_asks_for_the_domain_server_capability() {
        let toml = web_app_cargo_toml(&plan(&[Surface::Web]));
        assert!(
            toml.contains("domain = { path = \"../../domain\" }"),
            "domain must be a non-optional dependency: {toml}"
        );
        let domain_line = toml
            .lines()
            .find(|l| l.trim_start().starts_with("domain = "))
            .expect("a domain dependency line");
        assert!(
            !domain_line.contains("optional"),
            "domain must not be optional: {domain_line}"
        );

        let ssr = toml
            .lines()
            .find(|l| l.starts_with("ssr = "))
            .expect("an ssr feature line");
        assert!(
            ssr.contains("domain/server"),
            "the server build must ask for the domain's server capability: {ssr}"
        );

        let hydrate = toml
            .lines()
            .find(|l| l.starts_with("hydrate = "))
            .expect("a hydrate feature line");
        assert!(
            !hydrate.contains("domain/server") && !hydrate.contains("domain/sync-server"),
            "hydrate must not turn on a domain server feature: {hydrate}"
        );
    }
}
