//! Adding a surface to the `App` an app's `lib.rs` declares, with its handlers and feature.

use crate::Surface;
use crate::commands::new::{ssr_gate, surface_items, surface_line};
use crate::commands::scaffold::ProjectPlan;

/// The line in `app()` the builder lines follow.
const APP_START: &str = "    fundamental::app!()\n";

/// `lib` with `kind` added to its `App`; `None` without an `app()` built by `fundamental::app!()`.
pub(crate) fn add_surface(lib: &str, app: &str, kind: Surface, web: bool) -> Option<String> {
    let line = surface_line(kind, web)?;
    let start = lib.find("pub fn app() -> fundamental::App<")?;
    let at = start + lib[start..].find(APP_START)? + APP_START.len();
    let mut added = format!("{}{line}{}", &lib[..at], &lib[at..]);
    if kind == Surface::Api && !added[start..].contains(".state(") {
        let end = start + added[start..].find("\n}\n")?;
        added.insert_str(end, "\n        .state(|_| Ok(()))");
    }
    let mut surfaces = vec![kind];
    if web {
        surfaces.push(Surface::Web);
    }
    let plan = ProjectPlan::new(app, std::path::PathBuf::from(app), &surfaces).with_app(app);
    let gate = if web { ssr_gate() } else { "" };
    let items = surface_items(&plan, gate);
    let mut fresh = String::new();
    let mut lines = items.lines().peekable();
    while let Some(next) = lines.next() {
        let import = if next == gate.trim_end() && !gate.is_empty() {
            lines
                .peek()
                .filter(|line| line.starts_with("use "))
                .copied()
        } else {
            Some(next).filter(|line| line.starts_with("use "))
        };
        match import {
            Some(import) if added.contains(import) => {
                if next != import {
                    lines.next();
                }
            }
            _ => {
                fresh.push_str(next);
                fresh.push('\n');
            }
        }
    }
    added.push('\n');
    added.push_str(&fresh);
    Some(added)
}

/// The app crate's manifest asking `fundamental` for `kind`'s feature (in `ssr` for a web app).
pub(crate) fn add_feature(manifest: &str, kind: Surface, web: bool) -> Option<String> {
    let feature = match kind {
        Surface::Worker => "nats",
        Surface::Socket => "ws",
        Surface::Api => "api",
        Surface::Sync => "sync_server",
        Surface::Web | Surface::Native => return None,
    };
    let (list, quoted) = if web {
        ("ssr = [", format!("\"fundamental/{feature}\""))
    } else {
        (
            "fundamental = { workspace = true, features = [",
            format!("\"{feature}\""),
        )
    };
    let at = manifest.find(list)? + list.len();
    let end = at + manifest[at..].find(']')?;
    if manifest[at..end].contains(&quoted) {
        return None;
    }
    Some(format!("{}{quoted}, {}", &manifest[..at], &manifest[at..]))
}

/// The app crate's manifest asking the domain crate for what `kind` serves (`sync-server`).
pub(crate) fn add_domain_feature(manifest: &str, kind: Surface, web: bool) -> Option<String> {
    if kind != Surface::Sync {
        return None;
    }
    let (start, list, quoted) = if web {
        (0, "ssr = [", "\"domain/sync-server\"")
    } else {
        let line = manifest
            .find("\ndomain = {")
            .map(|at| at + 1)
            .or_else(|| manifest.starts_with("domain = {").then_some(0))?;
        (line, "features = [", "\"sync-server\"")
    };
    let line_end = start
        + manifest[start..]
            .find('\n')
            .unwrap_or(manifest.len() - start);
    let at = start + manifest[start..line_end].find(list)? + list.len();
    let end = at + manifest[at..line_end].find(']')?;
    if manifest[at..end].contains(quoted) {
        return None;
    }
    Some(format!("{}{quoted}, {}", &manifest[..at], &manifest[at..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKER: &str = "use fundamental::nats::{Message, NatsResult, ReceivedMessage};\n\
        use fundamental::subscribe;\n\
        use serde::{Deserialize, Serialize};\n\n\
        pub fn app() -> fundamental::App<()> {\n\
        \x20   fundamental::app!()\n\
        \x20       .worker(register_workers)\n\
        \x20       .messages(describe_messages)\n\
        }\n";

    #[test]
    fn a_surface_adds_its_one_line_after_the_app() {
        let added =
            add_surface(WORKER, "main", Surface::Socket, false).expect("an app() to add to");
        assert!(
            added.contains(
                "    fundamental::app!()\n        .socket(|router| router.route(on_ping_route()))\n"
            ),
            "{added}"
        );
        assert!(added.contains("async fn on_ping("), "{added}");
        assert_eq!(
            added.matches("use fundamental::subscribe;").count(),
            1,
            "{added}"
        );
        assert_eq!(added.matches("use serde::").count(), 1, "{added}");
        syn::parse_file(&added).unwrap_or_else(|e| panic!("{e}\n{added}"));
    }

    #[test]
    fn an_api_added_to_a_worker_serves_over_a_state() {
        let added = add_surface(WORKER, "main", Surface::Api, false).expect("an app() to add to");
        assert!(added.contains("        .api(register_api)\n"), "{added}");
        assert!(added.contains(".state(|_| Ok(()))\n}"), "{added}");
        assert!(added.contains("fundamental::mount_actions! {}"), "{added}");
        syn::parse_file(&added).unwrap_or_else(|e| panic!("{e}\n{added}"));
    }

    #[test]
    fn a_web_apps_surface_is_gated_to_its_server_build() {
        let web = "#[cfg(feature = \"ssr\")]\npub fn app() -> fundamental::App<()> {\n    fundamental::app!()\n        .state(|_| Ok(()))\n}\n";
        let added = add_surface(web, "main", Surface::Worker, true).expect("an app() to add to");
        assert!(
            added.contains("#[cfg(feature = \"ssr\")]\nfundamental::mount_workers! { on_hello }"),
            "{added}"
        );
        syn::parse_file(&added).unwrap_or_else(|e| panic!("{e}\n{added}"));
    }

    #[test]
    fn a_library_with_no_app_is_left_to_its_developer() {
        assert!(add_surface("fn main() {}\n", "main", Surface::Worker, false).is_none());
        assert!(add_surface(WORKER, "main", Surface::Native, false).is_none());
    }

    #[test]
    fn a_surface_asks_for_its_feature_once() {
        let app = "fundamental = { workspace = true, features = [\"app\", \"nats\"] }\n";
        let added = add_feature(app, Surface::Socket, false).expect("a list to add to");
        assert!(
            added.contains("features = [\"ws\", \"app\", \"nats\"]"),
            "{added}"
        );
        assert!(add_feature(app, Surface::Worker, false).is_none());

        let web = "ssr = [\"fundamental/web\", \"fundamental/app\"]\n";
        let added = add_feature(web, Surface::Api, true).expect("a list to add to");
        assert!(
            added.contains("ssr = [\"fundamental/api\", \"fundamental/web\""),
            "{added}"
        );
    }

    #[test]
    fn a_sync_surface_asks_the_domain_for_its_slices_once() {
        let app = "fundamental = { workspace = true, features = [\"app\", \"sql\"] }\n\
                   domain = { path = \"../../domain\", features = [\"server\"] }\n";
        let added = add_domain_feature(app, Surface::Sync, false).expect("a list to add to");
        assert!(
            added.contains(
                "domain = { path = \"../../domain\", features = [\"sync-server\", \"server\"] }"
            ),
            "{added}"
        );
        assert!(add_domain_feature(&added, Surface::Sync, false).is_none());
        assert!(add_domain_feature(app, Surface::Worker, false).is_none());

        let web = "ssr = [\"fundamental/web\", \"domain/server\"]\n";
        let added = add_domain_feature(web, Surface::Sync, true).expect("a list to add to");
        assert!(
            added.contains("ssr = [\"domain/sync-server\", \"fundamental/web\""),
            "{added}"
        );
        assert!(add_domain_feature(&added, Surface::Sync, true).is_none());
    }
}
