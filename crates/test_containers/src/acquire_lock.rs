use std::fs::{File, OpenOptions, create_dir_all};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tracing::{debug, warn};

/// How long a process waits before asking whether the holder is still alive.
///
/// Generous, because a cold engine's first boot takes nearly two minutes.
const LOCK_BUDGET: Duration = Duration::from_secs(180);

const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// A cross-process mutex held for the duration of one container acquisition.
///
/// `cargo test` runs binaries in parallel, and racing creates of one named container get a
/// 409 that `testcontainers` does not retry. The lock is a `create_new` file removed on `Drop`;
/// every failure here proceeds without it, since the 409 retry in
/// [`start_container`](crate::start_container) is what makes the race survivable.
#[derive(Debug)]
pub(crate) struct AcquireLock {
    path: Option<PathBuf>,
}

impl AcquireLock {
    /// Takes the lock named `key`, waiting up to [`LOCK_BUDGET`] for a holder to release it.
    #[expect(
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub(crate) fn take(key: &str) -> Self {
        let Some(path) = lock_path(key) else {
            warn!(
                key,
                "test containers: no lock directory; acquiring unserialised"
            );
            return Self { path: None };
        };
        if let Some(parent) = path.parent()
            && let Err(error) = create_dir_all(parent)
        {
            warn!(key, %error, "test containers: no lock directory; acquiring unserialised");
            return Self { path: None };
        }

        let mut deadline = Instant::now() + LOCK_BUDGET;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let _ = writeln!(file, "{}", std::process::id());
                    debug!(key, path = %path.display(), "test containers: lock taken");
                    return Self { path: Some(path) };
                }
                // A holder known to have exited is not waited for: a killed run leaves its locks.
                Err(_) if holder_has_exited(&path) => {
                    warn!(
                        key, path = %path.display(),
                        "test containers: lock holder has exited; breaking its abandoned lock"
                    );
                    if holder_has_exited(&path) {
                        let _ = std::fs::remove_file(&path);
                    }
                }
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(POLL_INTERVAL);
                }
                Err(error) => {
                    // Past the budget, but a slow holder is not an absent one: the budget only
                    // decides when to ask, and the holder's pid answers.
                    if holder_is_alive(&path) {
                        std::thread::sleep(POLL_INTERVAL);
                        deadline = Instant::now() + LOCK_BUDGET;
                        continue;
                    }
                    warn!(
                        key, %error, path = %path.display(),
                        "test containers: lock holder is gone; breaking an abandoned lock"
                    );
                    let _ = std::fs::remove_file(&path);
                    return match File::create(&path) {
                        Ok(mut file) => {
                            let _ = writeln!(file, "{}", std::process::id());
                            Self { path: Some(path) }
                        }
                        Err(error) => {
                            warn!(key, %error, "test containers: acquiring unserialised");
                            Self { path: None }
                        }
                    };
                }
            }
        }
    }
}

#[cfg(all(feature = "sql", debug_assertions))]
impl AcquireLock {
    /// Takes the lock named `key` if it is free **right now**, or reports that it is not.
    ///
    /// For leasing one of several interchangeable resources. An abandoned lock is broken by
    /// `remove` then `create_new`, so only one of two finders wins it.
    pub(crate) fn try_take(key: &str) -> Option<Self> {
        let path = lock_path(key)?;
        if let Some(parent) = path.parent()
            && create_dir_all(parent).is_err()
        {
            return None;
        }
        match Self::create_exclusively(&path) {
            Some(lock) => Some(lock),
            None if holder_is_alive(&path) => None,
            None => {
                let _ = std::fs::remove_file(&path);
                Self::create_exclusively(&path)
            }
        }
    }

    fn create_exclusively(path: &std::path::Path) -> Option<Self> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .ok()?;
        let _ = writeln!(file, "{}", std::process::id());
        debug!(path = %path.display(), "test containers: lock taken");
        Some(Self {
            path: Some(path.to_path_buf()),
        })
    }
}

impl Drop for AcquireLock {
    fn drop(&mut self) {
        // Never panics: a failing fixture may already be unwinding.
        if let Some(path) = &self.path
            && let Err(error) = std::fs::remove_file(path)
        {
            eprintln!(
                "test containers: could not release the lock at {} ({error}); \
                 the next run will break it after {}s",
                path.display(),
                LOCK_BUDGET.as_secs()
            );
        }
    }
}

/// Where the lock for `key` lives.
///
/// Scoped to the machine, as the containers, catalog and host databases it guards are:
/// `XDG_RUNTIME_DIR`, else `HOME/.cache`, else the temporary directory, each under the boot id
/// so a lock from a previous boot cannot name a recycled pid.
fn lock_path(key: &str) -> Option<PathBuf> {
    Some(lock_root()?.join(format!("{key}.lock")))
}

pub(crate) fn lock_root() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".cache"))
        })
        .unwrap_or_else(std::env::temp_dir);
    Some(root.join("fundamental-test-containers").join(boot_id()))
}

/// This boot, as a short directory-safe token; `unknown` without `/proc`.
fn boot_id() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|id| {
            id.trim()
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .take(16)
                .collect::<String>()
        })
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Whether `path` names another process that is certainly no longer running.
///
/// A file with no pid yet, or naming this process, is not known to be abandoned.
fn holder_has_exited(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| contents.trim().parse::<u32>().ok())
        .is_some_and(|pid| {
            pid != std::process::id() && !PathBuf::from(format!("/proc/{pid}")).is_dir()
        })
}

/// Whether the process that wrote `path` is still running.
///
/// A file with no readable pid, or a platform without `/proc`, reports not alive, so the lock
/// can always be cleared.
fn holder_is_alive(path: &std::path::Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(pid) = contents.trim().parse::<u32>() else {
        return false;
    };
    if pid == std::process::id() {
        // Our own pid means the file outlived an earlier run whose pid was recycled.
        return false;
    }
    PathBuf::from(format!("/proc/{pid}")).is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_released_lock_can_be_taken_again() {
        let first = AcquireLock::take("unit-release");
        let path = first.path.clone().expect("a lock path under target/");
        assert!(path.exists(), "the lock file must exist while held");
        drop(first);
        assert!(!path.exists(), "the lock file must be gone once released");

        let second = AcquireLock::take("unit-release");
        assert!(second.path.is_some(), "the lock must be retakeable");
    }

    #[cfg(all(feature = "sql", debug_assertions))]
    #[test]
    fn try_take_refuses_a_lock_another_live_process_holds() {
        // While a holder is alive, nobody else gets the resource. The holder is pid 1: certainly
        // running, and not ours.
        let path = lock_path("unit-try-live").expect("a lock path under target/");
        if let Some(parent) = path.parent() {
            create_dir_all(parent).expect("the lock directory");
        }
        std::fs::write(&path, "1\n").expect("write a live holder's lock");

        assert!(
            AcquireLock::try_take("unit-try-live").is_none(),
            "a live holder's lock must not be handed out a second time"
        );
        let contents = std::fs::read_to_string(&path).expect("the lock file is readable");
        assert_eq!(
            contents.trim(),
            "1",
            "refusing must leave the holder's own record intact rather than stamping over it"
        );
        std::fs::remove_file(&path).expect("clean up");
    }

    #[cfg(all(feature = "sql", debug_assertions))]
    #[test]
    fn a_released_lock_is_free_to_try_again() {
        let held = AcquireLock::try_take("unit-try-release").expect("a free lock is taken");
        let path = held.path.clone().expect("a lock path under target/");
        assert!(path.exists(), "the lock file must exist while held");
        drop(held);
        assert!(!path.exists(), "the lock file must be gone once released");
        assert!(
            AcquireLock::try_take("unit-try-release").is_some(),
            "and the resource must be leasable again"
        );
    }

    #[cfg(all(feature = "sql", debug_assertions))]
    #[test]
    fn try_take_reclaims_a_lock_whose_holder_is_gone() {
        // A `static` lease never drops, so a finished process's lock must read as free.
        let path = lock_path("unit-try-abandoned").expect("a lock path under target/");
        if let Some(parent) = path.parent() {
            create_dir_all(parent).expect("the lock directory");
        }
        std::fs::write(&path, "4294967294\n").expect("write an abandoned lock");

        let taken = AcquireLock::try_take("unit-try-abandoned")
            .expect("a lock whose holder is gone is free");
        let contents = std::fs::read_to_string(&path).expect("the lock file is readable");
        assert_eq!(
            contents.trim(),
            std::process::id().to_string(),
            "reclaiming rewrites the holder, so the next reader is told who has it now"
        );
        drop(taken);
    }

    #[test]
    fn the_lock_path_does_not_move_with_the_build_directory() {
        // The lock is scoped to the machine, not to `CARGO_TARGET_DIR`.
        let path = lock_path("unit-scope").expect("a lock path");
        assert!(
            !path.starts_with(
                std::env::var_os("CARGO_TARGET_DIR")
                    .map_or_else(|| PathBuf::from("target"), PathBuf::from)
            ),
            "the lock at {} is inside the build directory, so two checkouts sharing one \
             container would not share its lock",
            path.display()
        );
        assert!(
            path.to_string_lossy()
                .contains("fundamental-test-containers"),
            "{} must be recognisable as this harness's",
            path.display()
        );
    }

    #[test]
    fn a_lock_from_a_previous_boot_is_not_found_at_all() {
        let root = lock_root().expect("a lock root");
        let boot = root.file_name().expect("the root is keyed by something");
        assert_eq!(
            boot.to_string_lossy(),
            boot_id(),
            "the lock root must be keyed by this boot"
        );
    }

    #[test]
    fn a_lock_whose_holder_has_exited_is_taken_without_waiting() {
        let path = lock_path("unit-exited").expect("a lock path");
        if let Some(parent) = path.parent() {
            create_dir_all(parent).expect("the lock directory");
        }
        std::fs::write(&path, "4294967294\n").expect("write an abandoned lock");
        let started = Instant::now();
        let taken = AcquireLock::take("unit-exited");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "took {:?} to break a dead holder's lock",
            started.elapsed()
        );
        let contents = std::fs::read_to_string(&path).expect("the lock file is readable");
        assert_eq!(contents.trim(), std::process::id().to_string());
        drop(taken);
    }

    #[test]
    fn a_lock_with_no_pid_yet_is_not_known_to_be_abandoned() {
        // An empty file is a holder between `create_new` and `writeln!`.
        let path = lock_path("unit-unwritten").expect("a lock path");
        if let Some(parent) = path.parent() {
            create_dir_all(parent).expect("the lock directory");
        }
        std::fs::write(&path, "").expect("write an unwritten lock");
        assert!(!holder_has_exited(&path));
        std::fs::write(&path, format!("{}\n", std::process::id())).expect("our own pid");
        assert!(!holder_has_exited(&path));
        std::fs::write(&path, "4294967294\n").expect("a gone pid");
        assert!(holder_has_exited(&path));
        std::fs::remove_file(&path).expect("clean up");
    }

    #[test]
    fn a_lock_names_the_process_holding_it() {
        let held = AcquireLock::take("unit-pid");
        let path = held.path.clone().expect("a lock path under target/");
        let contents = std::fs::read_to_string(&path).expect("the lock file is readable");
        assert_eq!(contents.trim(), std::process::id().to_string());
    }
}
