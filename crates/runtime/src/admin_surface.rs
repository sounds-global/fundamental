//! The operational surface every service exposes ([`AdminSurface`]).

use std::collections::BTreeMap;
use std::net::SocketAddr;

use fundamental_core::listen::Listen;
use std::sync::Arc;

use fundamental_api::ApiServer;
use fundamental_core::{AnyError, ReadinessProbe, ServiceDeclaration, ServiceState};
use fundamental_server::{Method, Route, Router};
use serde::Serialize;
use tracing::{info, warn};

use crate::admin_error::AdminError;

/// Renders the process's metrics in whatever exposition format the deployment scrapes.
pub type MetricsRenderer = Arc<dyn Fn() -> Result<String, AnyError> + Send + Sync>;

const METRICS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Distinct from the API surface's `"api"` so the drain log tells the two listeners apart.
const SURFACE_NAME: &str = "admin";

/// The operational endpoints a supervised service exposes on its own address: liveness
/// (`/health`, touches nothing), readiness (`/ready`, runs every probe) and metrics.
pub struct AdminSurface {
    listen: Listen,
    state: ServiceState,
    declaration: Arc<dyn ServiceDeclaration>,
    probes: Vec<Box<dyn ReadinessProbe>>,
    metrics: Option<MetricsRenderer>,
}

#[derive(Clone)]
struct AdminState {
    service: ServiceState,
    declaration: Arc<dyn ServiceDeclaration>,
    probes: Arc<Vec<Box<dyn ReadinessProbe>>>,
    metrics: Option<MetricsRenderer>,
}

#[derive(Debug, Serialize)]
struct Liveness {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct Readiness {
    status: &'static str,
    service: String,
    probes: BTreeMap<String, &'static str>,
    declaration: BTreeMap<String, String>,
}

impl AdminSurface {
    /// The conventional path for a liveness probe.
    pub const HEALTH_PATH: &'static str = "/health";
    /// The conventional path for a readiness probe.
    pub const READY_PATH: &'static str = "/ready";
    /// The conventional path for a metrics scrape.
    pub const METRICS_PATH: &'static str = "/metrics";

    /// Creates an admin surface bound to `addr`, reporting on `state`.
    #[must_use]
    pub fn new(
        addr: SocketAddr,
        state: ServiceState,
        declaration: impl ServiceDeclaration,
    ) -> Self {
        Self::listening(Listen::Address(addr), state, declaration)
    }

    /// Creates an admin surface that listens where `listen` says.
    #[must_use]
    pub fn listening(
        listen: Listen,
        state: ServiceState,
        declaration: impl ServiceDeclaration,
    ) -> Self {
        Self {
            listen,
            state,
            declaration: Arc::new(declaration),
            probes: Vec::new(),
            metrics: None,
        }
    }

    /// Creates an admin surface where the app's `admin` surface listens: its address, or
    /// the socket passed as `admin` when it declares `listen = "activated"`.
    ///
    /// ```toml
    /// [apps.layer0]
    /// admin = { host = "127.0.0.1", port = 16100 }
    /// ```
    ///
    /// # Errors
    /// [`ConfigError`](fundamental_config::ConfigError) if the app declares no `admin` surface,
    /// declares no port for its own address, or the host/port pair is not a socket address.
    pub fn from_settings(
        app: &fundamental_config::AppSettings,
        state: ServiceState,
        declaration: impl ServiceDeclaration,
    ) -> Result<Self, fundamental_config::ConfigError> {
        Ok(Self::listening(app.admin_listen()?, state, declaration))
    }

    /// The state this surface reports on: what `/health` and `/ready` read.
    #[must_use]
    pub fn state(&self) -> &ServiceState {
        &self.state
    }

    /// Registers a dependency whose reachability gates readiness.
    #[must_use]
    pub fn probe(mut self, probe: impl ReadinessProbe) -> Self {
        self.probes.push(Box::new(probe));
        self
    }

    /// Registers the metrics renderer served at [`METRICS_PATH`](Self::METRICS_PATH); without one it answers `404`.
    #[must_use]
    pub fn metrics<F>(mut self, render: F) -> Self
    where
        F: Fn() -> Result<String, AnyError> + Send + Sync + 'static,
    {
        self.metrics = Some(Arc::new(render));
        self
    }

    /// Builds the supervised surface, ready to hand to a [`Runtime`](crate::Runtime).
    ///
    /// # Errors
    /// [`AnyError`] if a route cannot be registered.
    pub fn into_surface(self) -> Result<impl crate::Surface, AnyError> {
        info!(
            service = self.declaration.service(),
            declaration = ?self.declaration.rendered(),
            "service declaration bound to the admin surface"
        );

        let state = AdminState {
            service: self.state,
            declaration: self.declaration,
            probes: Arc::new(self.probes),
            metrics: self.metrics,
        };

        let router = Router::<AdminState>::new()
            .route(Route::get(Self::HEALTH_PATH, health))
            .and_then(|router| router.route(Route::get(Self::READY_PATH, ready)))
            .and_then(|router| {
                router.route(Route::action_raw(
                    Method::Get,
                    Self::METRICS_PATH,
                    METRICS_CONTENT_TYPE,
                    metrics,
                ))
            })
            .map_err(AnyError::new)?;

        // Not `Intake` like other `ApiServer`s: `/ready` must keep reporting the drain until the end.
        Ok(crate::PhasedSurface::new(
            crate::DrainPhase::Observability,
            crate::NamedSurface::new(
                SURFACE_NAME,
                ApiServer::new(
                    fundamental_server::ServerConfig::listening(self.listen),
                    state,
                    router,
                ),
            ),
        ))
    }
}

/// Liveness touches no dependency, so a slow one never restarts the process.
async fn health(
    context: fundamental_server::HttpContext<AdminState>,
    _input: (),
) -> Result<Liveness, AnyError> {
    if context.state().service.is_draining() {
        return Err(AnyError::new(AdminError::Draining));
    }
    Ok(Liveness { status: "ok" })
}

async fn ready(
    context: fundamental_server::HttpContext<AdminState>,
    _input: (),
) -> Result<Readiness, AnyError> {
    let state = context.state();
    if state.service.is_draining() {
        return Err(AnyError::new(AdminError::Draining));
    }

    let mut results = BTreeMap::new();
    let mut failed = Vec::new();
    for probe in state.probes.iter() {
        match probe.check().await {
            Ok(()) => {
                results.insert(probe.name().to_string(), "ok");
            }
            Err(error) => {
                warn!(probe = probe.name(), %error, "readiness probe failed");
                results.insert(probe.name().to_string(), "failed");
                failed.push(probe.name());
            }
        }
    }

    if !failed.is_empty() {
        return Err(AnyError::new(AdminError::NotReady {
            failed: failed.join(", "),
        }));
    }

    Ok(Readiness {
        status: "ready",
        service: state.declaration.service().to_string(),
        probes: results,
        declaration: state.declaration.rendered(),
    })
}

async fn metrics(
    context: fundamental_server::HttpContext<AdminState>,
    _input: (),
) -> Result<String, AnyError> {
    let Some(render) = context.state().metrics.clone() else {
        return Err(AnyError::new(AdminError::MetricsUnavailable));
    };
    render()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use fundamental_core::ProbeFuture;
    use fundamental_macros::Error;

    use super::*;
    use crate::Surface;

    #[derive(Debug, Error)]
    enum Unreachable {
        #[error(code = "TEST_001", message = "dependency unreachable", status = 503)]
        Dependency,
    }

    struct Probe {
        name: &'static str,
        up: bool,
        checks: Arc<AtomicUsize>,
    }

    impl Probe {
        fn new(name: &'static str, up: bool) -> (Self, Arc<AtomicUsize>) {
            let checks = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    name,
                    up,
                    checks: checks.clone(),
                },
                checks,
            )
        }
    }

    impl ReadinessProbe for Probe {
        fn name(&self) -> &'static str {
            self.name
        }

        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn check(&self) -> ProbeFuture<'_> {
            Box::pin(async move {
                self.checks.fetch_add(1, Ordering::SeqCst);
                if self.up {
                    Ok(())
                } else {
                    Err(AnyError::new(Unreachable::Dependency))
                }
            })
        }
    }

    struct Declaration;

    impl ServiceDeclaration for Declaration {
        fn service(&self) -> &str {
            "test-service"
        }

        fn rendered(&self) -> BTreeMap<String, String> {
            BTreeMap::from([("replicas".to_string(), "3".to_string())])
        }
    }

    fn state(service: ServiceState, probes: Vec<Box<dyn ReadinessProbe>>) -> AdminState {
        AdminState {
            service,
            declaration: Arc::new(Declaration),
            probes: Arc::new(probes),
            metrics: None,
        }
    }

    fn context(state: AdminState) -> fundamental_server::HttpContext<AdminState> {
        fundamental_server::HttpContext::new(state, "test-request")
    }

    #[tokio::test]
    async fn health_never_consults_a_probe() {
        let (probe, checks) = Probe::new("sql", false);
        let ready_state = ServiceState::new();
        ready_state.set_ready();

        let answer = health(context(state(ready_state, vec![Box::new(probe)])), ())
            .await
            .expect("liveness is unaffected by a failing dependency");

        assert_eq!(answer.status, "ok");
        assert_eq!(
            checks.load(Ordering::SeqCst),
            0,
            "liveness must not touch a dependency, even one that is down"
        );
    }

    #[tokio::test]
    async fn ready_reports_every_probe_by_name() {
        let (sql, _) = Probe::new("sql", true);
        let (cql, _) = Probe::new("cql", true);
        let service = ServiceState::new();
        service.set_ready();

        let answer = ready(
            context(state(service, vec![Box::new(sql), Box::new(cql)])),
            (),
        )
        .await
        .expect("every probe is up");

        assert_eq!(answer.status, "ready");
        assert_eq!(answer.probes.get("sql"), Some(&"ok"));
        assert_eq!(answer.probes.get("cql"), Some(&"ok"));
    }

    #[tokio::test]
    async fn ready_republishes_the_declaration() {
        let service = ServiceState::new();
        service.set_ready();

        let answer = ready(context(state(service, Vec::new())), ())
            .await
            .expect("a process with no probes is ready");

        assert_eq!(answer.service, "test-service");
        assert_eq!(
            answer.declaration.get("replicas").map(String::as_str),
            Some("3")
        );
    }

    #[tokio::test]
    async fn a_draining_instance_publishes_no_declaration() {
        let service = ServiceState::new();
        service.set_ready();
        service.set_draining();

        let error = ready(context(state(service, Vec::new())), ())
            .await
            .expect_err("a draining instance is never ready");

        assert_eq!(error.get().error_code(), "ADMIN_002");
    }

    #[tokio::test]
    async fn ready_names_the_failing_probe() {
        let (up, _) = Probe::new("sql", true);
        let (down, _) = Probe::new("cql", false);
        let service = ServiceState::new();
        service.set_ready();

        let error = ready(
            context(state(service, vec![Box::new(up), Box::new(down)])),
            (),
        )
        .await
        .expect_err("a probe is down");

        assert_eq!(error.get().error_code(), "ADMIN_001");
        assert_eq!(error.get().http_status_code(), 503);
        assert!(error.get().is_retryable());
        assert!(
            error.get().display_message().contains("cql"),
            "the response must name the failing probe; got {}",
            error.get().display_message()
        );
        assert!(
            !error.get().display_message().contains("sql"),
            "only the failing probe is named; got {}",
            error.get().display_message()
        );
    }

    #[tokio::test]
    async fn draining_short_circuits_readiness_without_probing() {
        let (probe, checks) = Probe::new("sql", true);
        let service = ServiceState::new();
        service.set_ready();
        service.set_draining();

        let error = ready(context(state(service, vec![Box::new(probe)])), ())
            .await
            .expect_err("a draining instance is never ready");

        assert_eq!(error.get().error_code(), "ADMIN_002");
        assert_eq!(error.get().http_status_code(), 503);
        assert_eq!(
            checks.load(Ordering::SeqCst),
            0,
            "a draining instance has nothing to prove; probing it only adds latency"
        );
    }

    #[tokio::test]
    async fn draining_also_fails_liveness_so_the_process_is_not_restarted_into_the_drain() {
        let service = ServiceState::new();
        service.set_ready();
        service.set_draining();

        let error = health(context(state(service, Vec::new())), ())
            .await
            .expect_err("a draining instance reports draining");

        assert_eq!(error.get().error_code(), "ADMIN_002");
    }

    #[tokio::test]
    async fn metrics_without_a_renderer_is_not_an_empty_scrape() {
        let service = ServiceState::new();
        service.set_ready();

        let error = metrics(context(state(service, Vec::new())), ())
            .await
            .expect_err("no renderer was registered");

        assert_eq!(error.get().error_code(), "ADMIN_003");
        assert_eq!(error.get().http_status_code(), 404);
    }

    #[tokio::test]
    async fn metrics_renders_what_the_service_registered() {
        let service = ServiceState::new();
        service.set_ready();
        let mut admin_state = state(service, Vec::new());
        admin_state.metrics = Some(Arc::new(|| Ok("acme_up 1\n".to_string())));

        let body = metrics(context(admin_state), ())
            .await
            .expect("the renderer succeeds");

        assert_eq!(body, "acme_up 1\n");
    }

    #[test]
    fn the_admin_listener_drains_last_not_with_the_other_api_surfaces() {
        let addr = "127.0.0.1:0".parse().expect("valid addr");
        let surface = AdminSurface::new(addr, ServiceState::new(), Declaration)
            .into_surface()
            .expect("the routes register");

        assert_eq!(surface.drain_phase(), crate::DrainPhase::Observability);
    }

    #[tokio::test]
    async fn the_surface_is_named_admin_not_api() {
        let addr = "127.0.0.1:0".parse().expect("valid addr");
        let surface = AdminSurface::new(addr, ServiceState::new(), Declaration)
            .into_surface()
            .expect("the routes register");

        assert_eq!(surface.name(), SURFACE_NAME);

        let guard = Box::new(surface)
            .start()
            .await
            .expect("binds an ephemeral port");
        assert_eq!(guard.name(), SURFACE_NAME);
        guard.stop().await.expect("stops cleanly");
    }

    #[test]
    fn from_settings_listens_where_the_admin_surface_says() {
        let dir = tempfile::tempdir().expect("a project");
        let manifest = dir.path().join("fun.toml");
        std::fs::write(
            &manifest,
            "[apps.main]\nadmin = { listen = \"activated\" }\nweb = { port = 9100 }\n",
        )
        .expect("write manifest");
        let app = fundamental_config::AppSettings::load_from("main", &manifest).expect("load app");

        let surface = AdminSurface::from_settings(&app, ServiceState::new(), Declaration)
            .expect("admin listens");

        assert_eq!(
            surface.listen,
            Listen::Activated(
                fundamental_core::listen::SocketName::parse("admin").expect("a socket name")
            )
        );
    }

    #[tokio::test]
    async fn a_runtime_with_an_admin_reports_its_lifecycle_on_the_admins_state() {
        let addr = "127.0.0.1:0".parse().expect("valid addr");
        let admin = AdminSurface::new(addr, ServiceState::new(), Declaration);
        let state = admin.state.clone();
        let running = state.clone();
        let ready_while_running = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = ready_while_running.clone();

        crate::Runtime::new()
            .with_admin(admin)
            .expect("the admin's routes register")
            .with_shutdown_signal(async move {
                seen.store(running.is_ready(), Ordering::SeqCst);
                Ok(())
            })
            .run()
            .await
            .expect("the runtime runs and drains");

        assert!(ready_while_running.load(Ordering::SeqCst));
        assert!(state.is_draining());
    }
}
