//! [`AppProcess`]: one supervised application process, with a graceful stop.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tracing::{debug, info, instrument, warn};

use crate::dev_error::DevError;
use crate::stop_outcome::StopOutcome;

const POLL: Duration = Duration::from_millis(25);

/// A running application, launched from its built binary, its output prefixed with its name.
#[derive(Debug)]
pub struct AppProcess {
    app: String,
    binary: PathBuf,
    child: Child,
    /// Joined on stop so no line is lost.
    pumps: Vec<JoinHandle<()>>,
}

impl AppProcess {
    /// Launch `binary` as `app`, with `env` applied on top of the inherited environment.
    ///
    /// # Errors
    /// [`DevError::AppLaunch`] if the process cannot be started.
    #[instrument(skip(env), fields(app, binary = %binary.display()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn spawn(
        app: &str,
        binary: &Path,
        env: &BTreeMap<String, String>,
    ) -> Result<Self, DevError> {
        let mut command = Command::new(binary);
        command
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|e| DevError::AppLaunch {
            app: app.to_string(),
            binary: binary.to_path_buf(),
            reason: e.to_string(),
        })?;

        let mut pumps = Vec::new();
        if let Some(out) = child.stdout.take() {
            pumps.push(pump(app.to_string(), out));
        }
        if let Some(err) = child.stderr.take() {
            pumps.push(pump(app.to_string(), err));
        }

        info!(app, pid = child.id(), "started");
        Ok(Self {
            app: app.to_string(),
            binary: binary.to_path_buf(),
            child,
            pumps,
        })
    }

    /// The app's name.
    #[must_use]
    pub fn app(&self) -> &str {
        &self.app
    }

    /// The binary this process is running.
    #[must_use]
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// Whether the process has exited on its own, and with what status.
    pub fn exited(&mut self) -> Option<ExitStatus> {
        match self.child.try_wait() {
            Ok(status) => status,
            // Treating an un-waitable child as running would hang the next stop.
            Err(e) => {
                warn!(app = %self.app, error = %e, "cannot poll the child; treating it as exited");
                None
            }
        }
    }

    /// Ask the process to stop: `SIGTERM`, then wait up to `grace`, then kill.
    #[instrument(skip(self), fields(app = %self.app), ret)]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn stop(mut self, grace: Duration) -> StopOutcome {
        if let Some(status) = self.exited() {
            self.join_pumps();
            return StopOutcome::AlreadyExited(status);
        }

        let started = Instant::now();
        request_termination(&self.child);

        while started.elapsed() < grace {
            if self.exited().is_some() {
                let took = started.elapsed();
                debug!(app = %self.app, ?took, "drained");
                self.join_pumps();
                return StopOutcome::Drained(took);
            }
            thread::sleep(POLL);
        }

        warn!(
            app = %self.app,
            grace_secs = grace.as_secs_f32(),
            "did not exit within the grace period; killing it (in-flight work is lost)"
        );
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.join_pumps();
        StopOutcome::Killed(started.elapsed())
    }

    /// Lets the last lines a stopping app wrote reach the terminal before the next message.
    fn join_pumps(&mut self) {
        for pump in self.pumps.drain(..) {
            let _ = pump.join();
        }
    }
}

/// `SIGTERM` via `kill(1)`: `libc::kill` needs `unsafe`, and `Child::kill` is `SIGKILL`.
#[cfg(unix)]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn request_termination(child: &Child) {
    let pid = child.id().to_string();
    // Already gone, or `kill` missing; the bounded wait below is the backstop.
    match Command::new("kill")
        .args(["-TERM", &pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => debug!(pid, code = status.code(), "kill -TERM did not succeed"),
        Err(e) => {
            warn!(pid, error = %e, "could not run kill(1); the process will be killed instead")
        }
    }
}

#[cfg(not(unix))]
fn request_termination(_child: &Child) {
    warn!("no SIGTERM on this platform; the process will be killed after the grace period");
}

/// Forward a child's stream to stderr in whole lines, so apps never interleave mid-line.
fn pump<R: std::io::Read + Send + 'static>(app: String, stream: R) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut sink = std::io::stderr();
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            let _ = writeln!(sink, "{app} │ {line}");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[cfg(unix)]
    fn script(dir: &TempDir, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.path().join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    #[test]
    fn launching_a_missing_binary_is_a_typed_error() {
        use fundamental_core::Error as _;
        let err = AppProcess::spawn("layer0", Path::new("/no/such/binary"), &BTreeMap::new())
            .expect_err("missing binary");
        assert_eq!(err.error_code(), "DEV_003");
    }

    /// Without waiting for the `trap`, the default disposition kills the shell and the test passes wrongly.
    #[cfg(unix)]
    fn await_ready(marker: &Path) {
        for _ in 0..400 {
            if marker.exists() {
                return;
            }
            thread::sleep(POLL);
        }
        panic!(
            "the child never signalled readiness at {}",
            marker.display()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_process_that_honours_sigterm_reports_a_drain() {
        let tmp = TempDir::new().expect("tempdir");
        let ready = tmp.path().join("ready");
        let bin = script(
            &tmp,
            "app",
            &format!(
                "trap 'exit 0' TERM\ntouch {}\nwhile true; do sleep 0.05; done",
                ready.display()
            ),
        );
        let proc = AppProcess::spawn("layer0", &bin, &BTreeMap::new()).expect("spawned");
        await_ready(&ready);

        let outcome = proc.stop(Duration::from_secs(5));
        assert!(matches!(outcome, StopOutcome::Drained(_)), "{outcome:?}");
        assert!(outcome.is_graceful(), "{outcome:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_process_that_ignores_sigterm_is_killed_and_says_so() {
        let tmp = TempDir::new().expect("tempdir");
        let ready = tmp.path().join("ready");
        let bin = script(
            &tmp,
            "stubborn",
            &format!(
                "trap '' TERM\ntouch {}\nwhile true; do sleep 0.05; done",
                ready.display()
            ),
        );
        let proc = AppProcess::spawn("layer5", &bin, &BTreeMap::new()).expect("spawned");
        await_ready(&ready);

        let outcome = proc.stop(Duration::from_millis(250));
        assert!(matches!(outcome, StopOutcome::Killed(_)), "{outcome:?}");
        assert!(!outcome.is_graceful(), "{outcome:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_process_that_exited_on_its_own_is_reported_as_such() {
        let tmp = TempDir::new().expect("tempdir");
        let bin = script(&tmp, "quitter", "exit 7");
        let mut proc = AppProcess::spawn("layer3", &bin, &BTreeMap::new()).expect("spawned");

        for _ in 0..200 {
            if proc.exited().is_some() {
                break;
            }
            thread::sleep(POLL);
        }
        let outcome = proc.stop(Duration::from_secs(1));
        match outcome {
            StopOutcome::AlreadyExited(status) => assert_eq!(status.code(), Some(7)),
            other => panic!("expected AlreadyExited, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_environment_reaches_the_child() {
        let tmp = TempDir::new().expect("tempdir");
        let out = tmp.path().join("seen");
        let bin = script(
            &tmp,
            "reader",
            &format!("printf '%s' \"$ACME_PROBE\" > {}", out.display()),
        );
        let mut env = BTreeMap::new();
        env.insert("ACME_PROBE".to_string(), "layer2-origin".to_string());

        let mut proc = AppProcess::spawn("layer2", &bin, &env).expect("spawned");
        for _ in 0..200 {
            if proc.exited().is_some() {
                break;
            }
            thread::sleep(POLL);
        }
        proc.stop(Duration::from_secs(1));
        assert_eq!(fs::read_to_string(&out).expect("wrote"), "layer2-origin");
    }
}
