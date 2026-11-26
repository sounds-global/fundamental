//! The app crate's `App`: each surface's handlers and one builder line in `pub fn app()`.

use crate::Surface;

use super::ProjectPlan;
use super::templates::sync_uses_cql;

/// The gate on a web app's server-only items; a non-web app's library needs none.
const SSR: &str = "#[cfg(feature = \"ssr\")]\n";

/// The app's `src/main.rs`, the same for every combination of surfaces.
pub(super) fn app_main_rs(plan: &ProjectPlan) -> String {
    format!(
        "//! `{app}` app, declared in `lib.rs`; `{app} check-config [manifest]` checks a release.\n\n\
         fn main() -> std::process::ExitCode {{\n\
         \x20   {crate_name}::app().run()\n\
         }}\n",
        app = plan.app_name,
        crate_name = plan.app_crate_name,
    )
}

/// A non-web app's `Cargo.toml`: the library declaring its `App` and the binary running it.
pub(super) fn app_cargo_toml(plan: &ProjectPlan) -> String {
    let mut features = vec!["\"app\""];
    features.extend(surface_features(plan));
    for engine in &plan.databases {
        features.push(match engine.feature() {
            "cql" => "\"cql\"",
            _ => "\"sql\"",
        });
    }
    let features = features.join(", ");
    let domain_dep = plan.domain.dependency(2, &plan.domain_features());
    format!(
        "[package]\n\
         name = \"{crate_name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\n\
         [lints]\n\
         workspace = true\n\n\
         [[bin]]\n\
         name = \"{crate_name}\"\n\
         path = \"src/main.rs\"\n\n\
         [dependencies]\n\
         fundamental = {{ workspace = true, features = [{features}] }}\n\
         {domain_dep}\n\
         serde = {{ workspace = true }}\n\
         tracing = {{ workspace = true }}\n\n\
         [features]\n\
         # Enabled by `fun dev`, never by `fun build`.\n\
         introspect = [\"fundamental/introspect\"]\n",
        crate_name = plan.app_crate_name,
    )
}

/// The umbrella features each of the plan's surfaces needs beside `app`.
fn surface_features(plan: &ProjectPlan) -> Vec<&'static str> {
    let mut features = Vec::new();
    if plan.worker {
        features.push("\"nats\"");
    }
    if plan.socket {
        features.push("\"ws\"");
    }
    if plan.api {
        features.push("\"api\"");
    }
    if plan.sync {
        features.push("\"sync_server\"");
    }
    features
}

/// A non-web app's `src/lib.rs`: each surface's handlers and the `App` they are declared in.
pub(super) fn app_lib_rs(plan: &ProjectPlan) -> String {
    format!(
        "//! `{app}` app: its surfaces, declared once as the `App` that `main.rs` runs.\n\n\
         {surfaces}\n\
         {app_fn}",
        app = plan.app_name,
        surfaces = surface_items(plan, ""),
        app_fn = app_fn(plan, ""),
    )
}

/// The imports and handlers of every surface but the pages, each item preceded by `gate`.
pub(crate) fn surface_items(plan: &ProjectPlan, gate: &str) -> String {
    let both = plan.worker && plan.socket;
    let mut imports = Vec::new();
    let mut defs = String::new();
    if plan.api {
        imports.push("use fundamental::AnyError;".to_owned());
        imports.push("use fundamental::api::{HttpContext, Route, Router};".to_owned());
        defs.push_str(&api_items(plan, gate));
    }
    if plan.worker {
        let (attr, alias) = if both {
            ("nats_subscribe", " as nats_subscribe")
        } else {
            ("subscribe", "")
        };
        imports.push("use fundamental::nats::{Message, NatsResult, ReceivedMessage};".to_owned());
        imports.push(format!("use fundamental::subscribe{alias};"));
        defs.push_str(&worker_items(attr, gate));
    }
    if plan.socket {
        let (attr, alias) = if both {
            ("ws_subscribe", " as ws_subscribe")
        } else {
            ("subscribe", "")
        };
        imports.push("use fundamental::ws::{WsContext, WsResult};".to_owned());
        imports.push(format!("use fundamental::subscribe{alias};"));
        defs.push_str(&socket_items(attr, gate));
    }
    if plan.worker || plan.socket || plan.api {
        imports.push("use serde::{Deserialize, Serialize};".to_owned());
    }
    if plan.sync {
        defs.push_str(&sync_items(gate));
    }
    let imports: String = imports
        .iter()
        .map(|import| format!("{gate}{import}\n"))
        .collect();
    format!("{imports}{defs}")
}

/// The API's types, handler and routes; on a non-web app also its `mount_actions!`.
fn api_items(plan: &ProjectPlan, gate: &str) -> String {
    let (mount, actions) = if plan.web {
        ("", "router")
    } else {
        (
            "\n// List each `domain::<Action>` this API serves.\n\
             fundamental::mount_actions! {}\n",
            "register_actions(router)?",
        )
    };
    format!(
        "{mount}\n\
         {gate}#[derive(Deserialize)]\n\
         struct Echo {{\n\
         \x20   message: String,\n\
         }}\n\n\
         {gate}#[derive(Serialize)]\n\
         struct Echoed {{\n\
         \x20   message: String,\n\
         }}\n\n\
         {gate}async fn echo(_context: HttpContext<()>, input: Echo) -> Result<Echoed, AnyError> {{\n\
         \x20   Ok(Echoed {{ message: input.message }})\n\
         }}\n\n\
         /// The API's routes, over the app's state.\n\
         {gate}fn register_api(router: Router<()>) -> Result<Router<()>, AnyError> {{\n\
         \x20   Ok({actions}.route(Route::post(\"/echo\", echo))?)\n\
         }}\n"
    )
}

/// The worker's message, its handler and the `mount_workers!` that lists it, with the chosen
/// `subscribe` attribute name (`subscribe`, or `nats_subscribe` beside a socket).
fn worker_items(attr: &str, gate: &str) -> String {
    format!(
        "\n{gate}#[derive(Serialize, Deserialize, Message)]\n\
         #[subject = \"events.hello\"]\n\
         struct Hello {{\n\
         \x20   name: String,\n\
         }}\n\n\
         // `#[subscribe]` accepts only a message mounted here.\n\
         {gate}fundamental::mount_messages! {{ Hello }}\n\n\
         {gate}#[{attr}]\n\
         async fn on_hello(message: ReceivedMessage<Hello>) -> NatsResult<()> {{\n\
         \x20   tracing::info!(name = %message.payload.name, \"received hello\");\n\
         \x20   Ok(())\n\
         }}\n\n\
         // List each `#[subscribe]` handler the worker runs.\n\
         {gate}fundamental::mount_workers! {{ on_hello }}\n"
    )
}

/// The socket's message types and handler, with the chosen `subscribe` attribute name
/// (`subscribe`, or `ws_subscribe` beside a worker).
fn socket_items(attr: &str, gate: &str) -> String {
    format!(
        "\n{gate}#[derive(Deserialize)]\n\
         struct Ping {{\n\
         \x20   n: u64,\n\
         }}\n\n\
         {gate}#[derive(Serialize)]\n\
         struct Pong {{\n\
         \x20   n: u64,\n\
         }}\n\n\
         {gate}#[{attr}(socket = \"ping\")]\n\
         async fn on_ping(context: WsContext, ping: Ping) -> WsResult<()> {{\n\
         \x20   context.send(Pong {{ n: ping.n + 1 }}).await\n\
         }}\n"
    )
}

/// The slices and projections the sync server serves, empty until `fun generate slice`.
fn sync_items(gate: &str) -> String {
    format!(
        "\n// List each `domain::sync::<Slice>` this surface serves.\n\
         {gate}fundamental::mount_slices! {{}}\n\n\
         // List each `domain::sync::<Projection>` this surface keeps fresh.\n\
         {gate}fundamental::mount_projections! {{}}\n"
    )
}

/// The builder line `kind` adds to `pub fn app()`, eight-space indented, as `fun new` writes
/// it and `fun add surface` adds it; `None` for a surface that is not one of the app's
/// (native is its own crate).
pub(crate) fn surface_line(kind: Surface, web: bool) -> Option<String> {
    let line = match kind {
        Surface::Web => {
            "        .pages(|router| register_actions(register_pages(router)?))\n".to_owned()
        }
        Surface::Api if web => "        .api(register_api)\n".to_owned(),
        Surface::Api => {
            "        .api(register_api)\n        .actions(describe_actions)\n".to_owned()
        }
        Surface::Worker => {
            "        .worker(register_workers)\n        .messages(describe_messages)\n".to_owned()
        }
        Surface::Socket => "        .socket(|router| router.route(on_ping_route()))\n\
             \x20       // Public until a route binds a caller with `WsContext::bind_principal`.\n\
             \x20       .socket_config(fundamental::ws::WsConfig::public)\n"
            .to_owned(),
        Surface::Sync => {
            "        .sync(register_slices)\n        .projections(register_projections)\n"
                .to_owned()
        }
        Surface::Native => return None,
    };
    Some(line)
}

/// The `App`'s sync store line and the note `app()`'s doc adds about it: a sync server over
/// CQL names its store when the app also declares SQL, which it would otherwise sync over.
fn sync_store(plan: &ProjectPlan) -> (&'static str, &'static str) {
    let over_cql = plan.sync && sync_uses_cql(plan);
    let beside_sql = plan
        .databases
        .iter()
        .any(|engine| engine.feature() != "cql");
    match (over_cql, beside_sql) {
        (true, true) => (
            "        .sync_store(fundamental::app::SyncStore::Cql)\n",
            "\n/// The sync server reads and writes the app's default CQL keyspace, not its SQL database.\n",
        ),
        (true, false) => (
            "",
            "\n/// The sync server reads and writes the app's default CQL keyspace.\n",
        ),
        _ => ("", ""),
    }
}

/// `pub fn app()`: the app's every surface as one value, each item preceded by `gate`.
pub(super) fn app_fn(plan: &ProjectPlan, gate: &str) -> String {
    let app = &plan.app_name;
    let mut lines = String::new();
    // With Tailwind, every page links the stylesheet its `css_command` builds, by version.
    let (head, links) = if plan.tailwind {
        (
            ".head(style_links)",
            format!(
                "\n/// The stylesheet every page links, built into `dist/app.css` by the `css_command`.\n\
                 {gate}fn style_links(ctx: &fundamental::web::PageContext) -> String {{\n\
                 \x20   format!(\n\
                 \x20       \"<link rel=\\\"stylesheet\\\" href=\\\"{{}}\\\">\",\n\
                 \x20       ctx.asset_url(\"/_fundamental/app.css\")\n\
                 \x20   )\n\
                 }}\n"
            ),
        )
    } else {
        ("", String::new())
    };
    if plan.web {
        lines.push_str(&surface_line(Surface::Web, true).unwrap_or_default());
        lines.push_str(&format!(
            "        .document(fundamental::web::Document::new().title(\"{app}\"){head})\n\
             \x20       .actions(describe_actions)\n"
        ));
    }
    for (present, kind) in [
        (plan.api, Surface::Api),
        (plan.worker, Surface::Worker),
        (plan.socket, Surface::Socket),
        (plan.sync, Surface::Sync),
    ] {
        if present {
            lines.push_str(&surface_line(kind, plan.web).unwrap_or_default());
        }
    }
    let (store_line, store) = sync_store(plan);
    lines.push_str(store_line);
    if plan.web || plan.api {
        lines.push_str("        .state(|_| Ok(()))\n");
    }
    let pages = if plan.web {
        format!(
            "\n// The pages, registered here so the server binary and the tests serve one router.\n\
             {gate}include!(concat!(env!(\"OUT_DIR\"), \"/pages.rs\"));\n"
        )
    } else {
        String::new()
    };
    format!(
        "{pages}\n\
         /// The app: every surface, as `main.rs` runs it and a test answers it.{store}\n\
         {gate}pub fn app() -> fundamental::App<()> {{\n\
         \x20   fundamental::app!()\n\
         {lines}\
         }}\n{links}"
    )
}

/// The `fundamental` features a web app's server build adds for its other surfaces.
pub(super) fn web_surface_features(plan: &ProjectPlan) -> Vec<&'static str> {
    surface_features(plan)
        .into_iter()
        .map(|feature| match feature {
            "\"nats\"" => "\"fundamental/nats\"",
            "\"ws\"" => "\"fundamental/ws\"",
            "\"api\"" => "\"fundamental/api\"",
            _ => "\"fundamental/sync_server\"",
        })
        .collect()
}

/// The gate for a web app's server-only items.
pub(crate) const fn ssr_gate() -> &'static str {
    SSR
}
