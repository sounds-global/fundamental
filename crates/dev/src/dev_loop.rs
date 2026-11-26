//! [`DevLoop`]: watch, rebuild, and restart only what changed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use tracing::{debug, error, info, instrument, warn};

use crate::app_process::AppProcess;
use crate::build_plan::BuildPlan;
use crate::dev_app::DevApp;
use crate::dev_error::DevError;
use crate::source_scan::SourceScan;

/// How long the tree must be quiet before a change triggers a rebuild.
const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(250);
/// How often the watched tree is re-scanned.
const DEFAULT_POLL: Duration = Duration::from_millis(300);
/// How long a restarted app is given to drain before it is killed.
const DEFAULT_GRACE: Duration = Duration::from_secs(10);

type PathTest = Box<dyn Fn(&Path) -> bool>;

/// The development loop: build the run set once, start it, then rebuild and restart on every change.
pub struct DevLoop {
    root: PathBuf,
    /// Where cargo writes the build, as cargo resolves it for `root`.
    target_dir: PathBuf,
    plan: BuildPlan,
    watch_roots: Vec<PathBuf>,
    debounce: Duration,
    poll: Duration,
    grace: Duration,
    watch: bool,
    prebuild: Option<Box<dyn Fn() -> Result<(), DevError>>>,
    alongside: Option<Box<dyn Fn() -> Result<(), DevError> + Send + Sync>>,
    style_only: Option<PathTest>,
}

impl DevLoop {
    /// A loop that builds `plan` from `root` and runs the binaries cargo writes to `target_dir`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, target_dir: impl Into<PathBuf>, plan: BuildPlan) -> Self {
        Self {
            root: root.into(),
            target_dir: target_dir.into(),
            plan,
            watch_roots: Vec::new(),
            debounce: DEFAULT_DEBOUNCE,
            poll: DEFAULT_POLL,
            grace: DEFAULT_GRACE,
            watch: true,
            prebuild: None,
            alongside: None,
            style_only: None,
        }
    }

    /// A step to run before every cargo build; its failure is treated like a compile error.
    #[must_use]
    pub fn prebuild(mut self, step: impl Fn() -> Result<(), DevError> + 'static) -> Self {
        self.prebuild = Some(Box::new(step));
        self
    }

    /// A step to run at the same time as every cargo build; its failure is treated like a compile error.
    #[must_use]
    pub fn alongside(
        mut self,
        step: impl Fn() -> Result<(), DevError> + Send + Sync + 'static,
    ) -> Self {
        self.alongside = Some(Box::new(step));
        self
    }

    /// Which changed paths the [`prebuild`](Self::prebuild) step alone answers, skipping cargo.
    #[must_use]
    pub fn style_only(mut self, answers: impl Fn(&Path) -> bool + 'static) -> Self {
        self.style_only = Some(Box::new(answers));
        self
    }

    /// The trees to watch; without any, the loop never rebuilds.
    #[must_use]
    pub fn watch_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.watch_roots = roots;
        self
    }

    /// How long the tree must be quiet before a change is acted on.
    #[must_use]
    pub fn debounce(mut self, window: Duration) -> Self {
        self.debounce = window;
        self
    }

    /// How often to re-scan the watched tree.
    #[must_use]
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll = interval;
        self
    }

    /// How long a restarting app may take to drain before it is killed.
    #[must_use]
    pub fn grace(mut self, grace: Duration) -> Self {
        self.grace = grace;
        self
    }

    /// Whether to watch for changes at all.
    #[must_use]
    pub fn watch(mut self, watch: bool) -> Self {
        self.watch = watch;
        self
    }

    /// Build the run set, start it, and supervise until interrupted.
    ///
    /// # Errors
    /// [`DevError::NothingToRun`], [`DevError::ToolLaunch`], [`DevError::UnwatchableRoot`] or
    /// [`DevError::AppLaunch`]; a first build that fails is fatal, every later failure is survived.
    #[instrument(skip(self), fields(apps = self.plan.apps().len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn run(self) -> Result<(), DevError> {
        if self.plan.is_empty() {
            return Err(DevError::NothingToRun);
        }

        let stop = install_interrupt_handler();

        info!(cargo = %format!("cargo {}", self.plan.cargo_args().join(" ")), "building the run set");
        if !self.build(&[])? {
            return Err(DevError::ToolLaunch {
                tool: "cargo".to_string(),
                reason: "the initial build failed; nothing was started".to_string(),
            });
        }

        let mut stamps = self.artifact_stamps();
        let mut running = self.start_all()?;

        if !self.watch || self.watch_roots.is_empty() {
            self.supervise_until_stopped(&mut running, &stop);
            return self.stop_all(running);
        }

        let mut scan = SourceScan::take(&self.watch_roots)?;
        if scan.is_empty() {
            warn!(
                roots = self.watch_roots.len(),
                "the watched roots contain no files; no rebuild can ever trigger"
            );
        }
        info!(files = scan.len(), "watching for changes");

        while !stop.load(Ordering::Relaxed) {
            thread::sleep(self.poll);
            self.report_exits(&mut running);

            let next = SourceScan::take(&self.watch_roots)?;
            let mut changes = scan.changes(&next);
            scan = next;
            if changes.is_empty() {
                continue;
            }

            scan = self.settle(scan, &mut changes, &stop)?;
            if stop.load(Ordering::Relaxed) {
                break;
            }

            info!(changed = changes.len(), first = ?changes.first(), "rebuilding");
            if !self.build(&changes)? {
                error!("build failed; the running apps were left untouched");
                continue;
            }

            let next_stamps = self.artifact_stamps();
            let restart: Vec<&DevApp> = self
                .plan
                .apps()
                .iter()
                .filter(|app| stamps.get(&app.name) != next_stamps.get(&app.name))
                .collect();
            stamps = next_stamps;

            if restart.is_empty() {
                info!("build produced no new binaries; nothing to restart");
                continue;
            }
            for app in restart {
                self.restart(app, &mut running);
            }
        }

        self.stop_all(running)
    }

    /// Wait until the tree is quiet for the debounce window, adding every change to `changes`.
    fn settle(
        &self,
        mut scan: SourceScan,
        changes: &mut Vec<PathBuf>,
        stop: &Arc<AtomicBool>,
    ) -> Result<SourceScan, DevError> {
        loop {
            thread::sleep(self.debounce);
            if stop.load(Ordering::Relaxed) {
                return Ok(scan);
            }
            let next = SourceScan::take(&self.watch_roots)?;
            let more = scan.changes(&next);
            let still_changing = !more.is_empty();
            changes.extend(more);
            scan = next;
            if !still_changing {
                return Ok(scan);
            }
            debug!("still changing; re-arming the debounce");
        }
    }

    /// `Ok(false)` means the compiler rejected the code, a normal event in a dev loop.
    fn build(&self, changes: &[PathBuf]) -> Result<bool, DevError> {
        let started = Instant::now();
        let built = self.build_for(changes, || self.cargo_build())?;
        if built {
            info!(secs = started.elapsed().as_secs_f32(), "build ok");
        }
        Ok(built)
    }

    fn prebuild_answers(&self, changes: &[PathBuf]) -> bool {
        match &self.style_only {
            Some(answers) if self.prebuild.is_some() && !changes.is_empty() => {
                changes.iter().all(|path| answers(path))
            }
            _ => false,
        }
    }

    fn build_for(
        &self,
        changes: &[PathBuf],
        cargo: impl FnOnce() -> Result<bool, DevError>,
    ) -> Result<bool, DevError> {
        if !self.prebuild_answers(changes) {
            return self.build_with(cargo);
        }
        match self.prebuild.as_ref().map(|prebuild| prebuild()) {
            Some(Err(e)) => {
                error!(error = %e, "the pre-build step failed");
                Ok(false)
            }
            _ => Ok(true),
        }
    }

    /// The prebuild step, then `cargo` and the alongside step at the same time.
    fn build_with(&self, cargo: impl FnOnce() -> Result<bool, DevError>) -> Result<bool, DevError> {
        if let Some(prebuild) = &self.prebuild
            && let Err(e) = prebuild()
        {
            error!(error = %e, "the pre-build step failed");
            return Ok(false);
        }
        match &self.alongside {
            None => cargo(),
            Some(alongside) => together(alongside, cargo),
        }
    }

    fn cargo_build(&self) -> Result<bool, DevError> {
        let status = Command::new("cargo")
            .args(self.plan.cargo_args())
            .envs(
                self.plan
                    .compile_envs()
                    .iter()
                    .map(|(key, value)| (key, value)),
            )
            .current_dir(&self.root)
            .status()
            .map_err(|e| DevError::ToolLaunch {
                tool: "cargo".to_string(),
                reason: e.to_string(),
            })?;
        Ok(status.success())
    }

    /// Every app's built-binary stamp; a missing binary stamps as absent, which counts as a change.
    fn artifact_stamps(&self) -> BTreeMap<String, (u64, SystemTime)> {
        let mut stamps = BTreeMap::new();
        for app in self.plan.apps() {
            let path = self.plan.binary_path(&self.target_dir, app);
            if let Some(stamp) = stamp(&path) {
                stamps.insert(app.name.clone(), stamp);
            }
        }
        stamps
    }

    fn start_all(&self) -> Result<BTreeMap<String, AppProcess>, DevError> {
        let mut running = BTreeMap::new();
        for app in self.plan.apps() {
            let binary = self.plan.binary_path(&self.target_dir, app);
            running.insert(
                app.name.clone(),
                AppProcess::spawn(&app.name, &binary, &app.env)?,
            );
        }
        Ok(running)
    }

    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn restart(&self, app: &DevApp, running: &mut BTreeMap<String, AppProcess>) {
        if let Some(process) = running.remove(&app.name) {
            let outcome = process.stop(self.grace);
            info!(app = %app.name, ?outcome, "stopped");
        }
        let binary = self.plan.binary_path(&self.target_dir, app);
        match AppProcess::spawn(&app.name, &binary, &app.env) {
            Ok(process) => {
                running.insert(app.name.clone(), process);
            }
            Err(e) => error!(app = %app.name, error = %e, "could not restart"),
        }
    }

    fn report_exits(&self, running: &mut BTreeMap<String, AppProcess>) {
        let exited: Vec<String> = running
            .iter_mut()
            .filter_map(|(name, process)| {
                process.exited().map(|status| {
                    warn!(
                        app = %name,
                        code = status.code().unwrap_or(-1),
                        "exited on its own; leaving it down until the next successful build"
                    );
                    name.clone()
                })
            })
            .collect();
        for name in exited {
            running.remove(&name);
        }
    }

    fn supervise_until_stopped(
        &self,
        running: &mut BTreeMap<String, AppProcess>,
        stop: &Arc<AtomicBool>,
    ) {
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(self.poll);
            self.report_exits(running);
            if running.is_empty() {
                warn!("every supervised app has exited");
                return;
            }
        }
    }

    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn stop_all(&self, running: BTreeMap<String, AppProcess>) -> Result<(), DevError> {
        for (name, process) in running {
            let outcome = process.stop(self.grace);
            if outcome.is_graceful() {
                info!(app = %name, ?outcome, "stopped");
            } else {
                warn!(app = %name, ?outcome, "stopped");
            }
        }
        Ok(())
    }
}

/// Both finish before either is judged, so a compile error never hides a failed step beside it.
fn together(
    alongside: &(dyn Fn() -> Result<(), DevError> + Send + Sync),
    cargo: impl FnOnce() -> Result<bool, DevError>,
) -> Result<bool, DevError> {
    let (built, beside) = thread::scope(|scope| {
        let beside = scope.spawn(alongside);
        let built = cargo();
        (built, beside.join())
    });
    let failure = match beside {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(e.to_string()),
        Err(_) => Some("it panicked".to_owned()),
    };
    if let Some(reason) = &failure {
        error!(error = %reason, "the step alongside the build failed");
    }
    Ok(built? && failure.is_none())
}

fn stamp(path: &Path) -> Option<(u64, SystemTime)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// A flag flipped on `SIGINT`/`SIGTERM`, so the supervisor waits for the apps' drains.
#[cfg(unix)]
fn install_interrupt_handler() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let raised = Arc::clone(&flag);
    thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(e) => {
                warn!(error = %e, "no signal watcher; Ctrl-C will not wait for drains");
                return;
            }
        };
        runtime.block_on(async {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut term), Ok(mut int)) = (
                signal(SignalKind::terminate()),
                signal(SignalKind::interrupt()),
            ) else {
                warn!("could not register SIGTERM/SIGINT; Ctrl-C will not wait for drains");
                return;
            };
            tokio::select! {
                _ = term.recv() => info!("SIGTERM; stopping the dev loop"),
                _ = int.recv() => info!("SIGINT; stopping the dev loop"),
            }
            raised.store(true, Ordering::Relaxed);
        });
    });
    flag
}

#[cfg(not(unix))]
fn install_interrupt_handler() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn an_empty_plan_is_refused_rather_than_watched() {
        use fundamental_core::Error as _;
        let err = DevLoop::new("/w", "/w/target", BuildPlan::new(Vec::new()))
            .run()
            .expect_err("nothing to run");
        assert_eq!(err.error_code(), "DEV_004");
    }

    #[test]
    fn the_builder_defaults_are_the_documented_ones() {
        let loop_ = DevLoop::new(
            "/w",
            "/w/target",
            BuildPlan::new(vec![DevApp::new("layer0")]),
        );
        assert_eq!(loop_.debounce, DEFAULT_DEBOUNCE);
        assert_eq!(loop_.poll, DEFAULT_POLL);
        assert_eq!(loop_.grace, DEFAULT_GRACE);
        assert!(loop_.watch);
    }

    #[test]
    fn the_builder_overrides_apply() {
        let loop_ = DevLoop::new(
            "/w",
            "/w/target",
            BuildPlan::new(vec![DevApp::new("layer0")]),
        )
        .debounce(Duration::from_millis(50))
        .poll_interval(Duration::from_millis(60))
        .grace(Duration::from_secs(3))
        .watch(false)
        .watch_roots(vec![PathBuf::from("/w/domain")]);
        assert_eq!(loop_.debounce, Duration::from_millis(50));
        assert_eq!(loop_.poll, Duration::from_millis(60));
        assert_eq!(loop_.grace, Duration::from_secs(3));
        assert!(!loop_.watch);
        assert_eq!(loop_.watch_roots.len(), 1);
    }

    /// Each side waits for the other, so run one after the other the first times out.
    fn meets(here: std::sync::mpsc::Sender<()>, there: &std::sync::mpsc::Receiver<()>) -> bool {
        let _ = here.send(());
        there.recv_timeout(Duration::from_secs(5)).is_ok()
    }

    #[test]
    fn the_alongside_step_runs_while_cargo_builds() {
        let (to_cargo, from_step) = std::sync::mpsc::channel();
        let (to_step, from_cargo) = std::sync::mpsc::channel();
        let from_step = std::sync::Mutex::new(from_step);
        let from_cargo = std::sync::Mutex::new(from_cargo);
        let to_cargo = std::sync::Mutex::new(Some(to_cargo));
        let loop_ = DevLoop::new("/w", "/w/target", BuildPlan::new(vec![DevApp::new("a")]))
            .alongside(move || {
                let here = to_cargo.lock().expect("lock").take().expect("once");
                if meets(here, &from_cargo.lock().expect("lock")) {
                    Ok(())
                } else {
                    Err(DevError::NothingToRun)
                }
            });
        let built = loop_
            .build_with(|| Ok(meets(to_step, &from_step.lock().expect("lock"))))
            .expect("the build ran");
        assert!(built, "both halves met, so they ran at the same time");
    }

    #[test]
    fn a_failing_alongside_step_fails_the_build() {
        let loop_ = DevLoop::new("/w", "/w/target", BuildPlan::new(vec![DevApp::new("a")]))
            .alongside(|| Err(DevError::NothingToRun));
        assert!(!loop_.build_with(|| Ok(true)).expect("the build ran"));
    }

    #[test]
    fn a_failing_cargo_build_fails_whatever_the_alongside_step_did() {
        let loop_ = DevLoop::new("/w", "/w/target", BuildPlan::new(vec![DevApp::new("a")]))
            .alongside(|| Ok(()));
        assert!(!loop_.build_with(|| Ok(false)).expect("the build ran"));
    }

    #[test]
    fn the_prebuild_step_runs_before_the_build_and_its_failure_stops_it() {
        let loop_ = DevLoop::new("/w", "/w/target", BuildPlan::new(vec![DevApp::new("a")]))
            .prebuild(|| Err(DevError::NothingToRun));
        let built = loop_
            .build_with(|| panic!("cargo must not run after a failed prebuild"))
            .expect("the build ran");
        assert!(!built);
    }

    #[test]
    fn a_change_to_stylesheets_alone_runs_only_the_prebuild_step() {
        let prebuilt = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = prebuilt.clone();
        let loop_ = DevLoop::new("/w", "/w/target", BuildPlan::new(vec![DevApp::new("a")]))
            .prebuild(move || {
                seen.set(true);
                Ok(())
            })
            .alongside(|| panic!("the islands must not build for a stylesheet"))
            .style_only(|path| path.extension().is_some_and(|ext| ext == "css"));
        let built = loop_
            .build_for(&[PathBuf::from("/w/domain/brand/styles/brand.css")], || {
                panic!("cargo must not run for a stylesheet")
            })
            .expect("the build ran");
        assert!(built);
        assert!(prebuilt.get(), "the stylesheet was rebuilt");
    }

    #[test]
    fn a_stylesheet_change_whose_styles_fail_reports_a_failed_build() {
        let loop_ = DevLoop::new("/w", "/w/target", BuildPlan::new(vec![DevApp::new("a")]))
            .prebuild(|| {
                Err(DevError::ToolLaunch {
                    tool: "tailwindcss".to_string(),
                    reason: "not found".to_string(),
                })
            })
            .alongside(|| panic!("the islands must not build for a stylesheet"))
            .style_only(|path| path.extension().is_some_and(|ext| ext == "css"));
        let built = loop_
            .build_for(&[PathBuf::from("/w/domain/brand/styles/brand.css")], || {
                panic!("cargo must not run for a stylesheet")
            })
            .expect("a failed step is a failed build, not an error");
        assert!(!built);
    }

    #[test]
    fn without_a_prebuild_step_a_stylesheet_change_runs_the_whole_build() {
        let loop_ = DevLoop::new("/w", "/w/target", BuildPlan::new(vec![DevApp::new("a")]))
            .style_only(|path| path.extension().is_some_and(|ext| ext == "css"));
        let cargo_ran = std::cell::Cell::new(false);
        let built = loop_
            .build_for(&[PathBuf::from("/w/domain/brand/styles/brand.css")], || {
                cargo_ran.set(true);
                Ok(true)
            })
            .expect("the build ran");
        assert!(built && cargo_ran.get());
    }

    #[test]
    fn a_change_beside_a_stylesheet_runs_the_whole_build() {
        let loop_ = DevLoop::new("/w", "/w/target", BuildPlan::new(vec![DevApp::new("a")]))
            .style_only(|path| path.extension().is_some_and(|ext| ext == "css"));
        let cargo_ran = std::cell::Cell::new(false);
        let built = loop_
            .build_for(
                &[
                    PathBuf::from("/w/domain/brand/styles/brand.css"),
                    PathBuf::from("/w/domain/brand/src/lib.rs"),
                ],
                || {
                    cargo_ran.set(true);
                    Ok(true)
                },
            )
            .expect("the build ran");
        assert!(built && cargo_ran.get());
    }

    #[test]
    fn a_missing_artifact_has_no_stamp() {
        assert!(stamp(Path::new("/no/such/binary")).is_none());
    }

    #[test]
    fn an_existing_artifact_stamps_its_length() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("layer0");
        fs::write(&path, b"binary bytes").expect("write");
        let (len, _) = stamp(&path).expect("stamped");
        assert_eq!(len, 12);
    }

    #[test]
    fn artifact_stamps_only_report_apps_whose_binary_exists() {
        let tmp = TempDir::new().expect("tempdir");
        let target = tmp.path().join("elsewhere");
        fs::create_dir_all(target.join("debug")).expect("target");
        fs::write(target.join("debug/layer0"), b"built").expect("layer0");
        fs::create_dir_all(tmp.path().join("target/debug")).expect("default target");
        fs::write(tmp.path().join("target/debug/layer1"), b"stale").expect("layer1");

        let plan = BuildPlan::new(vec![DevApp::new("layer0"), DevApp::new("layer1")]);
        let loop_ = DevLoop::new(tmp.path(), &target, plan);
        let stamps = loop_.artifact_stamps();

        assert!(stamps.contains_key("layer0"));
        assert!(
            !stamps.contains_key("layer1"),
            "an app unbuilt in the target directory must not stamp, whatever `<root>/target` holds"
        );
    }
}
