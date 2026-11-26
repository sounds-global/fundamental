//! The [`ProjectPlan`] describing exactly what to scaffold.

use crate::helpers::DomainCrate;
use std::path::PathBuf;

use crate::Surface;
use crate::engine::Engine;

pub(crate) const DEFAULT_APP: &str = "main";
pub(crate) const DEFAULT_SOCKET_PORT: u16 = 9000;
pub(crate) const DEFAULT_API_PORT: u16 = 16001;
pub(crate) const DEFAULT_WEB_PORT: u16 = 11001;
pub(crate) const DEFAULT_SYNC_PORT: u16 = 9100;

/// A fully-resolved description of the project to generate, the generator's single input.
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub(crate) struct ProjectPlan {
    /// As typed, already validated.
    pub name: String,
    /// Its directory under `apps/` and its `[apps.<name>]` key.
    pub app_name: String,
    /// `app_name` as the app crate's package identifier.
    pub app_crate_name: String,
    pub path: PathBuf,
    pub worker: bool,
    pub socket: bool,
    pub socket_port: u16,
    pub api: bool,
    pub api_port: u16,
    pub web: bool,
    pub web_port: u16,
    pub sync: bool,
    pub sync_port: u16,
    pub native: bool,
    /// Tailwind for the web surface: `styles/input.css`, a `css_command`, and a stylesheet link.
    pub tailwind: bool,
    /// The engines to wire up (config and crate feature only): none, YSQL, YCQL or both.
    pub databases: Vec<Engine>,
    /// The slice store's backend when several are wired; `None` defaults it (YSQL when present).
    pub sync_store: Option<Engine>,
    pub domain: DomainCrate,
}

impl ProjectPlan {
    /// A plan for the validated name, path and surfaces: app [`DEFAULT_APP`], no database.
    pub(crate) fn new(name: &str, path: PathBuf, surfaces: &[Surface]) -> Self {
        Self {
            name: name.to_string(),
            app_name: DEFAULT_APP.to_string(),
            app_crate_name: sanitize_crate_name(DEFAULT_APP),
            path,
            worker: surfaces.contains(&Surface::Worker),
            socket: surfaces.contains(&Surface::Socket),
            socket_port: DEFAULT_SOCKET_PORT,
            api: surfaces.contains(&Surface::Api),
            api_port: DEFAULT_API_PORT,
            web: surfaces.contains(&Surface::Web),
            web_port: DEFAULT_WEB_PORT,
            sync: surfaces.contains(&Surface::Sync),
            sync_port: DEFAULT_SYNC_PORT,
            native: surfaces.contains(&Surface::Native),
            tailwind: false,
            databases: Vec::new(),
            sync_store: None,
            domain: DomainCrate::scaffolded(),
        }
    }

    /// Depend on `domain` rather than the crate `fun new` scaffolds at `domain/`.
    #[must_use]
    pub(crate) fn with_domain(mut self, domain: DomainCrate) -> Self {
        self.domain = domain;
        self
    }

    pub(crate) fn domain_features(&self) -> Vec<&'static str> {
        let mut features = Vec::new();
        if self.has_app() || self.web {
            features.push("server");
        }
        if self.sync {
            features.push("sync-server");
        }
        features
    }

    #[must_use]
    pub(crate) fn with_sync_store(mut self, engine: Option<Engine>) -> Self {
        self.sync_store = engine;
        self
    }

    #[must_use]
    pub(crate) fn with_tailwind(mut self, tailwind: bool) -> Self {
        self.tailwind = tailwind;
        self
    }

    /// Set the app name and derive its crate identifier.
    #[must_use]
    pub(crate) fn with_app(mut self, app: &str) -> Self {
        self.app_name = app.to_string();
        self.app_crate_name = sanitize_crate_name(app);
        self
    }

    #[must_use]
    pub(crate) fn with_socket_port(mut self, port: u16) -> Self {
        self.socket_port = port;
        self
    }

    #[must_use]
    pub(crate) fn with_databases(mut self, databases: Vec<Engine>) -> Self {
        self.databases = databases;
        self
    }

    /// Whether any runtime surface was selected (otherwise: domain-only).
    pub(crate) fn has_app(&self) -> bool {
        self.worker || self.socket || self.api || self.web || self.sync || self.native
    }

    /// A wasm library beside `pages/`: a web app, or a native app whose webview runs the same bundle.
    pub(crate) fn is_web_shaped(&self) -> bool {
        self.web || self.native
    }

    /// A web app's islands live in `apps/<app>/islands/`, unless it syncs or runs native.
    pub(crate) fn has_islands_crate(&self) -> bool {
        self.web && !self.sync && !self.native
    }

    /// The app crate plus, for a native app, its `native` crate and `hooks`; shared by `fun new` and `fun add app`.
    pub(crate) fn workspace_members(&self) -> Vec<String> {
        let app = &self.app_name;
        let mut members = Vec::new();
        if self.has_app() {
            members.push(format!("apps/{app}"));
        }
        if self.has_islands_crate() {
            members.push(format!("apps/{app}/islands"));
        }
        if self.native {
            members.push(format!("apps/{app}/native"));
            members.push(format!("apps/{app}/native/hooks"));
        }
        members
    }

    pub(crate) fn surfaces(&self) -> Vec<Surface> {
        let flags = [
            (self.worker, Surface::Worker),
            (self.socket, Surface::Socket),
            (self.api, Surface::Api),
            (self.web, Surface::Web),
            (self.sync, Surface::Sync),
            (self.native, Surface::Native),
        ];
        flags
            .into_iter()
            .filter_map(|(present, surface)| present.then_some(surface))
            .collect()
    }
}

/// Hyphens, legal in a package name but not a Rust path, become underscores.
fn sanitize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyphens_become_underscores_in_the_app_crate_name() {
        let plan = ProjectPlan::new("my-app", PathBuf::from("my-app"), &[Surface::Worker])
            .with_app("my-svc");
        assert_eq!(plan.app_crate_name, "my_svc");
        assert_eq!(plan.name, "my-app");
    }

    #[test]
    fn no_surfaces_is_domain_only() {
        let plan = ProjectPlan::new("acme", PathBuf::from("acme"), &[]);
        assert!(!plan.has_app());
        assert!(!plan.worker);
        assert!(!plan.socket);
    }

    #[test]
    fn surface_flags_track_the_selection() {
        let plan = ProjectPlan::new("acme", PathBuf::from("acme"), &[Surface::Socket]);
        assert!(plan.has_app());
        assert!(plan.socket);
        assert!(!plan.worker);
    }

    #[test]
    fn defaults_are_the_main_app_on_the_default_port_with_no_database() {
        let plan = ProjectPlan::new("acme", PathBuf::from("acme"), &[Surface::Socket]);
        assert_eq!(plan.app_name, "main");
        assert_eq!(plan.app_crate_name, "main");
        assert_eq!(plan.socket_port, DEFAULT_SOCKET_PORT);
        assert!(plan.databases.is_empty());
    }

    #[test]
    fn builders_override_app_port_and_databases() {
        let plan = ProjectPlan::new("acme", PathBuf::from("acme"), &[Surface::Socket])
            .with_app("billing-svc")
            .with_socket_port(8080)
            .with_databases(vec![Engine::SqlDb, Engine::CqlDb]);
        assert_eq!(plan.app_name, "billing-svc");
        assert_eq!(plan.app_crate_name, "billing_svc");
        assert_eq!(plan.socket_port, 8080);
        assert_eq!(plan.databases, vec![Engine::SqlDb, Engine::CqlDb]);
    }

    #[test]
    fn a_web_app_registers_its_islands_crate() {
        let plan = ProjectPlan::new("acme", PathBuf::from("acme"), &[Surface::Web]).with_app("www");
        assert_eq!(
            plan.workspace_members(),
            vec!["apps/www".to_string(), "apps/www/islands".to_string()]
        );
    }

    #[test]
    fn a_syncing_or_native_app_keeps_its_islands_in_the_app_crate() {
        for surfaces in [
            vec![Surface::Web, Surface::Sync],
            vec![Surface::Web, Surface::Native],
        ] {
            let plan = ProjectPlan::new("acme", PathBuf::from("acme"), &surfaces).with_app("www");
            assert!(!plan.has_islands_crate(), "{surfaces:?}");
        }
    }

    #[test]
    fn workspace_members_registers_native_and_its_hooks() {
        let plan =
            ProjectPlan::new("acme", PathBuf::from("acme"), &[Surface::Native]).with_app("desk");
        assert_eq!(
            plan.workspace_members(),
            vec![
                "apps/desk".to_string(),
                "apps/desk/native".to_string(),
                "apps/desk/native/hooks".to_string(),
            ]
        );
    }
}
