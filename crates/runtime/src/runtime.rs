//! [`Runtime`] — supervises one or more surfaces under a single shutdown signal.

use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use fundamental_core::{AnyError, ServiceState};
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{error, info, instrument, warn};

use crate::{DrainPhase, RuntimeError, RuntimeResult, Surface, SurfaceGuard};

/// The tracing target every process-lifecycle line is emitted on, held to a floor by `fundamental_observability`.
pub const LIFECYCLE: &str = "fundamental::lifecycle";

type ShutdownFuture = Pin<Box<dyn Future<Output = std::io::Result<()>> + Send>>;

/// A supervisor that runs one or more [`Surface`]s under a single shutdown signal.
///
/// ```rust
/// # extern crate fundamental_runtime;
/// # use fundamental_runtime::Runtime;
/// # async fn run() -> fundamental_runtime::RuntimeResult<()> {
/// // let worker = Worker::new(client).with_listener(hello);
/// // let socket = WsServer::new(config, router);
/// Runtime::new()
/// //  .with(worker)
/// //  .with(socket)
///     .run()
///     .await
/// # }
/// ```
#[must_use = "a Runtime does nothing until `run` is awaited"]
pub struct Runtime {
    surfaces: Vec<Box<dyn Surface>>,
    shutdown_timeout: Option<Duration>,
    shutdown: Option<ShutdownFuture>,
    service_state: Option<ServiceState>,
}

impl Runtime {
    /// The default drain bound; it sits under the usual supervisor grace period so a hung surface is reported, not `SIGKILL`ed.
    pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

    /// Creates an empty runtime with the default termination signal and drain bound.
    pub fn new() -> Self {
        Self {
            surfaces: Vec::new(),
            shutdown_timeout: Some(Self::DEFAULT_SHUTDOWN_TIMEOUT),
            shutdown: None,
            service_state: None,
        }
    }

    /// Publishes this runtime's lifecycle phase into `state`: ready once every surface has
    /// started, draining before any surface stops.
    pub fn with_service_state(mut self, state: ServiceState) -> Self {
        self.service_state = Some(state);
        self
    }

    /// Registers the admin listener and publishes this runtime's lifecycle on the state its `/ready` reports.
    ///
    /// # Errors
    /// As [`AdminSurface::into_surface`](crate::AdminSurface::into_surface).
    #[cfg(feature = "api")]
    pub fn with_admin(
        self,
        admin: crate::AdminSurface,
    ) -> Result<Self, fundamental_core::AnyError> {
        let state = admin.state().clone();
        Ok(self.with_service_state(state).with(admin.into_surface()?))
    }

    /// Registers a surface to supervise.
    pub fn with(mut self, surface: impl Surface) -> Self {
        self.surfaces.push(Box::new(surface));
        self
    }

    /// Bounds how long the drain may take before remaining surfaces are force-stopped.
    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = Some(timeout);
        self
    }

    /// Overrides the shutdown trigger (default: [`termination_signal`](crate::termination_signal)).
    pub fn with_shutdown_signal<F>(mut self, signal: F) -> Self
    where
        F: Future<Output = std::io::Result<()>> + Send + 'static,
    {
        self.shutdown = Some(Box::pin(signal));
        self
    }

    /// Starts every surface, awaits the first of the shutdown signal or any
    /// surface ending, then drains every surface in registration order.
    ///
    /// # Errors
    /// - [`RuntimeError::Start`] if a surface fails to start (surfaces started
    ///   before it are drained first, in reverse).
    /// - [`RuntimeError::SurfaceFailed`] if a surface ends mid-run with an error.
    /// - [`RuntimeError::Signal`] if the shutdown-signal handler fails.
    /// - [`RuntimeError::DrainTimeout`] if a bounded drain elapses.
    /// - [`RuntimeError::DrainBudget`] if a surface needs a longer drain than the shutdown
    ///   budget; refused before any surface starts.
    #[instrument(skip(self), fields(surfaces = self.surfaces.len()))]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub async fn run(self) -> RuntimeResult<()> {
        let Self {
            surfaces,
            shutdown_timeout,
            shutdown,
            service_state,
        } = self;

        if let Some(budget) = shutdown_timeout {
            fits_the_budget(&surfaces, budget)?;
        }

        let mut guards: Vec<PhasedGuard> = Vec::with_capacity(surfaces.len());
        for surface in surfaces {
            let name = surface.name();
            let phase = surface.drain_phase();
            match surface.start().await {
                Ok(guard) => {
                    info!(
                        target: LIFECYCLE,
                        surface = name,
                        phase = phase.as_str(),
                        "surface started"
                    );
                    guards.push(PhasedGuard { phase, guard });
                }
                Err(source) => {
                    error!(target: LIFECYCLE, surface = name, error = %source, "surface failed to start; draining started surfaces");
                    if let Some(state) = &service_state {
                        state.set_draining();
                    }
                    // Nothing served yet, so strict reverse start order undoes the partial start, not phases.
                    guards.reverse();
                    if let Err(drain_err) = bounded(drain_in_order(guards), shutdown_timeout).await
                    {
                        warn!(target: LIFECYCLE, error = %drain_err, "drain error while unwinding a failed start");
                    }
                    return Err(RuntimeError::Start {
                        surface: name,
                        source,
                    });
                }
            }
        }

        if let Some(state) = &service_state {
            state.set_ready();
        }

        let shutdown: ShutdownFuture =
            shutdown.unwrap_or_else(|| Box::pin(crate::termination_signal()));
        let trigger = supervise(&mut guards, shutdown).await;

        // Stop being sent work before a single surface begins tearing down.
        if let Some(state) = &service_state {
            state.set_draining();
        }

        let terminal: RuntimeResult<()> = match trigger {
            Trigger::Signal(Ok(())) => {
                info!(
                    target: LIFECYCLE,
                    reason = "signal",
                    "shutdown signal received; draining surfaces"
                );
                Ok(())
            }
            Trigger::Signal(Err(source)) => {
                error!(target: LIFECYCLE, reason = "signal_error", error = %source, "shutdown signal handler failed; draining surfaces");
                Err(RuntimeError::Signal { source })
            }
            Trigger::Surface {
                name,
                result: Ok(()),
            } => {
                info!(
                    target: LIFECYCLE,
                    reason = "surface_ended",
                    surface = name,
                    "surface ended on its own; draining surfaces"
                );
                Ok(())
            }
            Trigger::Surface {
                name,
                result: Err(source),
            } => {
                warn!(target: LIFECYCLE, reason = "surface_failure", surface = name, error = %source, "surface failed; draining surfaces");
                Err(RuntimeError::SurfaceFailed {
                    surface: name,
                    source,
                })
            }
        };

        let drain_result = bounded(drain_by_phase(by_phase(guards)), shutdown_timeout).await;

        // A primary failure wins over a drain error during the same shutdown.
        match terminal {
            Err(primary) => {
                if let Err(drain_err) = drain_result {
                    warn!(target: LIFECYCLE, error = %drain_err, "drain error during shutdown, masked by the primary failure");
                }
                Err(primary)
            }
            Ok(()) => drain_result,
        }
    }

    #[cfg(test)]
    fn surface_count(&self) -> usize {
        self.surfaces.len()
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

struct PhasedGuard {
    phase: DrainPhase,
    guard: Box<dyn SurfaceGuard>,
}

/// Sorts guards by phase; the sort must stay stable, registration order within a phase is a dependency.
fn by_phase(mut guards: Vec<PhasedGuard>) -> Vec<PhasedGuard> {
    guards.sort_by_key(|entry| entry.phase);
    guards
}

/// What ended the supervise phase.
enum Trigger {
    Signal(std::io::Result<()>),
    Surface {
        name: &'static str,
        result: Result<(), AnyError>,
    },
}

/// Awaits the first of the shutdown signal or any surface ending on its own.
async fn supervise(guards: &mut [PhasedGuard], shutdown: ShutdownFuture) -> Trigger {
    let mut waits: Vec<(&'static str, _)> = guards
        .iter_mut()
        .map(|entry| {
            let name = entry.guard.name();
            (name, entry.guard.wait())
        })
        .collect();

    let any_surface = poll_fn(|cx| {
        for (name, wait) in &mut waits {
            if let Poll::Ready(result) = wait.as_mut().poll(cx) {
                return Poll::Ready(Trigger::Surface { name, result });
            }
        }
        Poll::Pending
    });

    let signal = async move { Trigger::Signal(shutdown.await) };

    tokio::select! {
        trigger = signal => trigger,
        trigger = any_surface => trigger,
    }
}

/// Refuses surfaces whose drains, run as [`drain_by_phase`] runs them, need `budget` or more.
fn fits_the_budget(surfaces: &[Box<dyn Surface>], budget: Duration) -> RuntimeResult<()> {
    let declared = |phase: DrainPhase| {
        surfaces
            .iter()
            .filter(move |surface| surface.drain_phase() == phase)
            .filter_map(|surface| Some((surface.name(), surface.drain_budget()?)))
    };
    let mut needed = Duration::ZERO;
    let longest_intake = declared(DrainPhase::Intake).max_by_key(|(_, drain)| *drain);
    let in_sequence = DrainPhase::ORDER
        .into_iter()
        .filter(|phase| *phase != DrainPhase::Intake)
        .flat_map(declared);
    for (surface, drain) in longest_intake.into_iter().chain(in_sequence) {
        needed = needed.saturating_add(drain);
        if needed >= budget {
            let error = RuntimeError::DrainBudget {
                surface,
                needed_ms: millis(needed),
                budget_ms: millis(budget),
            };
            error!(target: LIFECYCLE, surface, %error, "the drain does not fit the shutdown budget");
            return Err(error);
        }
    }
    Ok(())
}

/// Stops every intake surface at once, so none keeps accepting through another's drain, then the rest in order.
async fn drain_by_phase(guards: Vec<PhasedGuard>) {
    let (intake, rest): (Vec<_>, Vec<_>) = guards
        .into_iter()
        .partition(|entry| entry.phase == DrainPhase::Intake);
    let mut stopping = JoinSet::new();
    for PhasedGuard { phase, guard } in intake {
        let name = guard.name();
        stopping.spawn(async move { (name, phase, guard.stop().await) });
    }
    while let Some(stopped) = stopping.join_next().await {
        match stopped {
            Ok((name, phase, result)) => report_drained(name, phase, result),
            Err(error) => {
                warn!(target: LIFECYCLE, %error, "an intake surface's drain panicked");
            }
        }
    }
    drain_in_order(rest).await;
}

async fn drain_in_order(guards: Vec<PhasedGuard>) {
    for PhasedGuard { phase, guard } in guards {
        let name = guard.name();
        report_drained(name, phase, guard.stop().await);
    }
}

#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn report_drained(name: &'static str, phase: DrainPhase, result: Result<(), AnyError>) {
    match result {
        Ok(()) => info!(
            target: LIFECYCLE,
            surface = name,
            phase = phase.as_str(),
            "surface drained"
        ),
        Err(error) => {
            warn!(target: LIFECYCLE, surface = name, phase = phase.as_str(), %error, "surface drain error")
        }
    }
}

/// Runs `drain` under an optional bound; on timeout the unstopped guards are dropped.
async fn bounded(drain: impl Future<Output = ()>, budget: Option<Duration>) -> RuntimeResult<()> {
    match budget {
        None => {
            drain.await;
            Ok(())
        }
        Some(budget) => match timeout(budget, drain).await {
            Ok(()) => Ok(()),
            Err(_elapsed) => {
                let timeout_ms = millis(budget);
                warn!(
                    target: LIFECYCLE,
                    timeout_ms,
                    "shutdown drain timed out; remaining surfaces force-stopped"
                );
                Err(RuntimeError::DrainTimeout { timeout_ms })
            }
        },
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crate::{DrainFuture, StartFuture, WaitFuture};
    use fundamental_core::Error as _;
    use tracing_test::traced_test;

    #[derive(Clone)]
    struct Recorder(Arc<Mutex<Vec<&'static str>>>);

    impl Recorder {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Vec::new())))
        }
        fn drained(&self) -> Vec<&'static str> {
            self.0.lock().expect("recorder lock").clone()
        }
    }

    #[derive(Clone, Copy)]
    enum Behavior {
        Idle,
        EndsOk,
        EndsErr,
        StartErr,
        StopHangs,
    }

    struct FakeSurface {
        name: &'static str,
        behavior: Behavior,
        drained: Recorder,
    }

    impl FakeSurface {
        fn new(name: &'static str, behavior: Behavior, drained: Recorder) -> Self {
            Self {
                name,
                behavior,
                drained,
            }
        }
    }

    impl Surface for FakeSurface {
        fn name(&self) -> &'static str {
            self.name
        }

        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn start(self: Box<Self>) -> StartFuture {
            let Self {
                name,
                behavior,
                drained,
            } = *self;
            Box::pin(async move {
                match behavior {
                    Behavior::StartErr => {
                        Err(AnyError::new(RuntimeError::DrainTimeout { timeout_ms: 7 }))
                    }
                    _ => Ok(Box::new(FakeGuard {
                        name,
                        behavior,
                        drained,
                    }) as Box<dyn SurfaceGuard>),
                }
            })
        }
    }

    struct FakeGuard {
        name: &'static str,
        behavior: Behavior,
        drained: Recorder,
    }

    impl SurfaceGuard for FakeGuard {
        fn name(&self) -> &'static str {
            self.name
        }

        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn wait(&mut self) -> WaitFuture<'_> {
            let behavior = self.behavior;
            Box::pin(async move {
                match behavior {
                    Behavior::EndsOk => Ok(()),
                    Behavior::EndsErr => {
                        Err(AnyError::new(RuntimeError::DrainTimeout { timeout_ms: 99 }))
                    }
                    _ => std::future::pending().await,
                }
            })
        }

        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn stop(self: Box<Self>) -> DrainFuture {
            let Self {
                name,
                behavior,
                drained,
            } = *self;
            Box::pin(async move {
                if let Behavior::StopHangs = behavior {
                    std::future::pending::<()>().await;
                }
                drained.0.lock().expect("recorder lock").push(name);
                Ok(())
            })
        }
    }

    async fn fire() -> std::io::Result<()> {
        Ok(())
    }

    struct SlowIntake {
        name: &'static str,
        stop_takes: Duration,
        budget: Option<Duration>,
        started: Recorder,
        drained: Recorder,
    }

    impl SlowIntake {
        fn new(
            name: &'static str,
            stop_takes: Duration,
            started: &Recorder,
            drained: &Recorder,
        ) -> Self {
            Self {
                name,
                stop_takes,
                budget: Some(stop_takes),
                started: started.clone(),
                drained: drained.clone(),
            }
        }
    }

    impl Surface for SlowIntake {
        fn name(&self) -> &'static str {
            self.name
        }

        fn drain_phase(&self) -> DrainPhase {
            DrainPhase::Intake
        }

        fn drain_budget(&self) -> Option<Duration> {
            self.budget
        }

        fn start(self: Box<Self>) -> StartFuture {
            Box::pin(async move {
                self.started
                    .0
                    .lock()
                    .expect("recorder lock")
                    .push(self.name);
                Ok(Box::new(SlowGuard {
                    name: self.name,
                    stop_takes: self.stop_takes,
                    drained: self.drained,
                }) as Box<dyn SurfaceGuard>)
            })
        }
    }

    struct SlowGuard {
        name: &'static str,
        stop_takes: Duration,
        drained: Recorder,
    }

    impl SurfaceGuard for SlowGuard {
        fn name(&self) -> &'static str {
            self.name
        }

        fn wait(&mut self) -> WaitFuture<'_> {
            Box::pin(std::future::pending())
        }

        fn stop(self: Box<Self>) -> DrainFuture {
            Box::pin(async move {
                tokio::time::sleep(self.stop_takes).await;
                self.drained
                    .0
                    .lock()
                    .expect("recorder lock")
                    .push(self.name);
                Ok(())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn intake_surfaces_drain_together_so_their_drains_do_not_add_up() {
        let (started, drained) = (Recorder::new(), Recorder::new());
        let twenty = Duration::from_secs(20);
        Runtime::new()
            .with(SlowIntake::new("api", twenty, &started, &drained))
            .with(SlowIntake::new("socket", twenty, &started, &drained))
            .with_shutdown_timeout(Duration::from_secs(30))
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect("two 20 s drains fit a 30 s budget when they run together");
        let mut both = drained.drained();
        both.sort_unstable();
        assert_eq!(both, ["api", "socket"]);
    }

    #[tokio::test]
    async fn a_surface_whose_drain_does_not_fit_the_budget_is_refused_before_anything_starts() {
        let (started, drained) = (Recorder::new(), Recorder::new());
        let error = Runtime::new()
            .with(FakeSurface::new("worker", Behavior::Idle, Recorder::new()))
            .with(SlowIntake::new(
                "socket",
                Duration::from_secs(31),
                &started,
                &drained,
            ))
            .with_shutdown_timeout(Duration::from_secs(30))
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect_err("a 31 s drain cannot fit a 30 s budget");
        assert!(matches!(
            error,
            RuntimeError::DrainBudget {
                surface: "socket",
                needed_ms: 31_000,
                budget_ms: 30_000
            }
        ));
        assert_eq!(error.error_code(), "RUN_005");
        assert_eq!(error.http_status_code(), 500);
        assert!(
            started.drained().is_empty(),
            "nothing starts when a drain cannot fit"
        );
    }

    #[tokio::test]
    async fn drains_run_in_sequence_add_up_against_the_budget() {
        let (started, drained) = (Recorder::new(), Recorder::new());
        let twenty = Duration::from_secs(20);
        let error = Runtime::new()
            .with(SlowIntake::new("api", twenty, &started, &drained))
            .with(crate::PhasedSurface::new(
                DrainPhase::Work,
                SlowIntake::new("relay", Duration::from_secs(15), &started, &drained),
            ))
            .with_shutdown_timeout(Duration::from_secs(30))
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect_err("20 s of intake then 15 s of work cannot fit 30 s");
        assert!(matches!(
            error,
            RuntimeError::DrainBudget {
                surface: "relay",
                needed_ms: 35_000,
                budget_ms: 30_000
            }
        ));
        assert!(started.drained().is_empty());
    }

    #[tokio::test]
    async fn a_renamed_or_rephased_surface_keeps_its_drain_budget() {
        let (started, drained) = (Recorder::new(), Recorder::new());
        let named = crate::NamedSurface::new(
            "public",
            SlowIntake::new("socket", Duration::from_secs(30), &started, &drained),
        );
        let phased = crate::PhasedSurface::new(
            DrainPhase::Work,
            SlowIntake::new("socket", Duration::from_secs(30), &started, &drained),
        );
        assert_eq!(named.drain_budget(), Some(Duration::from_secs(30)));
        assert_eq!(phased.drain_budget(), Some(Duration::from_secs(30)));
        let error = Runtime::new()
            .with(named)
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect_err("a drain equal to the budget leaves nothing for the rest");
        assert_eq!(error.error_code(), "RUN_005");
    }

    #[tokio::test]
    async fn with_accumulates_surfaces() {
        let rec = Recorder::new();
        let rt = Runtime::new()
            .with(FakeSurface::new("a", Behavior::Idle, rec.clone()))
            .with(FakeSurface::new("b", Behavior::Idle, rec));
        assert_eq!(rt.surface_count(), 2);
    }

    #[tokio::test]
    #[traced_test]
    async fn signal_triggers_clean_shutdown_and_drains_in_order() {
        let rec = Recorder::new();
        Runtime::new()
            .with(FakeSurface::new("worker", Behavior::Idle, rec.clone()))
            .with(FakeSurface::new("socket", Behavior::Idle, rec.clone()))
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect("clean shutdown");

        assert_eq!(rec.drained(), vec!["worker", "socket"]);
        assert!(logs_contain("shutdown signal received"));
    }

    #[tokio::test]
    async fn phases_drain_before_registration_order_does() {
        let rec = Recorder::new();
        Runtime::new()
            .with(crate::PhasedSurface::new(
                DrainPhase::Observability,
                FakeSurface::new("admin", Behavior::Idle, rec.clone()),
            ))
            .with(FakeSurface::new("outbox", Behavior::Idle, rec.clone()))
            .with(crate::PhasedSurface::new(
                DrainPhase::Intake,
                FakeSurface::new("api", Behavior::Idle, rec.clone()),
            ))
            .with(FakeSurface::new("relay", Behavior::Idle, rec.clone()))
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect("clean shutdown");

        assert_eq!(
            rec.drained(),
            vec!["api", "outbox", "relay", "admin"],
            "intake closes first, work drains in registration order, \
             and the surface that reports on the drain goes last"
        );
    }

    #[tokio::test]
    async fn a_failed_start_still_unwinds_in_reverse_regardless_of_phase() {
        let rec = Recorder::new();
        let _ = Runtime::new()
            .with(crate::PhasedSurface::new(
                DrainPhase::Observability,
                FakeSurface::new("admin", Behavior::Idle, rec.clone()),
            ))
            .with(crate::PhasedSurface::new(
                DrainPhase::Intake,
                FakeSurface::new("api", Behavior::Idle, rec.clone()),
            ))
            .with(FakeSurface::new("boom", Behavior::StartErr, rec.clone()))
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect_err("the third surface fails to start");

        assert_eq!(
            rec.drained(),
            vec!["api", "admin"],
            "the started surfaces unwind newest-first, not by phase"
        );
    }

    // `fundamental_observability` repeats this literal for its floor and cannot depend on this crate.
    #[test]
    fn the_lifecycle_target_matches_the_floor_observability_applies() {
        assert_eq!(LIFECYCLE, "fundamental::lifecycle");
    }

    #[tokio::test]
    async fn start_failure_unwinds_started_surfaces_in_reverse() {
        let rec = Recorder::new();
        let err = Runtime::new()
            .with(FakeSurface::new("a", Behavior::Idle, rec.clone()))
            .with(FakeSurface::new("b", Behavior::Idle, rec.clone()))
            .with(FakeSurface::new("c", Behavior::StartErr, rec.clone()))
            .with(FakeSurface::new("d", Behavior::Idle, rec.clone()))
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect_err("start fails");

        assert!(matches!(err, RuntimeError::Start { surface: "c", .. }));
        assert_eq!(err.error_code(), "RUN_001");
        assert_eq!(rec.drained(), vec!["b", "a"]);
    }

    #[tokio::test]
    #[traced_test]
    async fn mid_run_failure_brings_the_process_down() {
        let rec = Recorder::new();
        let err = Runtime::new()
            .with(FakeSurface::new("socket", Behavior::EndsErr, rec.clone()))
            .with(FakeSurface::new("worker", Behavior::Idle, rec.clone()))
            .with_shutdown_signal(std::future::pending::<std::io::Result<()>>())
            .run()
            .await
            .expect_err("surface fails mid-run");

        match err {
            RuntimeError::SurfaceFailed { surface, source } => {
                assert_eq!(surface, "socket");
                assert_eq!(source.get().error_code(), "RUN_004");
            }
            other => panic!("expected SurfaceFailed, got {other:?}"),
        }
        assert!(logs_contain("surface_failure"));
    }

    #[tokio::test]
    async fn clean_self_end_shuts_down_without_error() {
        let rec = Recorder::new();
        Runtime::new()
            .with(FakeSurface::new("oneshot", Behavior::EndsOk, rec.clone()))
            .with_shutdown_signal(std::future::pending::<std::io::Result<()>>())
            .run()
            .await
            .expect("a clean self-end is not an error");
        assert_eq!(rec.drained(), vec!["oneshot"]);
    }

    #[tokio::test(start_paused = true)]
    async fn drain_timeout_force_stops_a_stuck_surface() {
        let rec = Recorder::new();
        let err = Runtime::new()
            .with(FakeSurface::new("stuck", Behavior::StopHangs, rec))
            .with_shutdown_signal(fire())
            .with_shutdown_timeout(Duration::from_millis(50))
            .run()
            .await
            .expect_err("drain exceeds the bound");

        assert!(matches!(err, RuntimeError::DrainTimeout { timeout_ms: 50 }));
        assert_eq!(err.error_code(), "RUN_004");
    }

    #[tokio::test]
    async fn draining_is_published_before_any_surface_stops() {
        struct PhaseAtStop {
            state: ServiceState,
            observed: Arc<Mutex<Option<fundamental_core::Phase>>>,
        }

        struct PhaseGuard {
            state: ServiceState,
            observed: Arc<Mutex<Option<fundamental_core::Phase>>>,
        }

        impl Surface for PhaseAtStop {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn name(&self) -> &'static str {
                "phase-at-stop"
            }
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn start(self: Box<Self>) -> StartFuture {
                Box::pin(async move {
                    Ok(Box::new(PhaseGuard {
                        state: self.state,
                        observed: self.observed,
                    }) as Box<dyn SurfaceGuard>)
                })
            }
        }

        impl SurfaceGuard for PhaseGuard {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn name(&self) -> &'static str {
                "phase-at-stop"
            }
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn wait(&mut self) -> WaitFuture<'_> {
                Box::pin(std::future::pending())
            }
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn stop(self: Box<Self>) -> DrainFuture {
                Box::pin(async move {
                    *self.observed.lock().expect("observed lock") = Some(self.state.phase());
                    Ok(())
                })
            }
        }

        let state = ServiceState::new();
        let observed = Arc::new(Mutex::new(None));

        Runtime::new()
            .with_service_state(state.clone())
            .with(PhaseAtStop {
                state: state.clone(),
                observed: observed.clone(),
            })
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect("a clean shutdown");

        assert_eq!(
            *observed.lock().expect("observed lock"),
            Some(fundamental_core::Phase::Draining),
            "the surface must already see `Draining` when it is asked to stop"
        );
    }

    #[tokio::test]
    async fn readiness_is_published_only_after_every_surface_starts() {
        let state = ServiceState::new();
        let rec = Recorder::new();

        assert!(!state.is_ready(), "not ready before `run`");

        Runtime::new()
            .with_service_state(state.clone())
            .with(FakeSurface::new("a", Behavior::Idle, rec))
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect("a clean shutdown");

        assert!(state.is_draining());
    }

    #[tokio::test(start_paused = true)]
    async fn a_drain_is_bounded_by_default() {
        let rec = Recorder::new();
        let err = Runtime::new()
            .with(FakeSurface::new("stuck", Behavior::StopHangs, rec))
            .with_shutdown_signal(fire())
            .run()
            .await
            .expect_err("the default bound still force-stops a stuck surface");

        let expected = Runtime::DEFAULT_SHUTDOWN_TIMEOUT.as_millis() as u64;
        assert!(matches!(
            err,
            RuntimeError::DrainTimeout { timeout_ms } if timeout_ms == expected
        ));
    }

    #[tokio::test]
    async fn signal_handler_failure_is_reported() {
        let err = Runtime::new()
            .with_shutdown_signal(async { Err(std::io::Error::other("no handler")) })
            .run()
            .await
            .expect_err("signal handler failed");
        assert!(matches!(err, RuntimeError::Signal { .. }));
        assert_eq!(err.error_code(), "RUN_003");
    }
}
