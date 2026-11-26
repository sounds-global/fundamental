use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use tracing::{debug, warn};

use crate::acquire_lock::{AcquireLock, lock_root};

/// Performs `rite` exactly once for `key` in this test run, and makes every other process
/// wait for it to finish.
///
/// Keyed on `NEXTEST_RUN_ID`, since nextest runs one process per test, else on the process id.
/// Never fails the caller.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub async fn once_per_run<F, Fut>(key: &str, rite: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    // Held across the rite, so queued processes resume after it.
    let _lock = AcquireLock::take(key);

    let Some(marker) = marker_path(key) else {
        warn!(
            key,
            "test containers: no marker directory; performing this rite unconditionally"
        );
        rite().await;
        return;
    };

    if marker.exists() {
        debug!(
            key,
            "test containers: this run's rite has already been performed"
        );
        return;
    }

    rite().await;

    // Written after the rite, so a process killed mid-rite leaves no marker.
    if let Some(parent) = marker.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        warn!(key, %error, "test containers: could not record that the rite ran");
        return;
    }
    if let Err(error) = std::fs::write(&marker, std::process::id().to_string()) {
        warn!(key, %error, "test containers: could not record that the rite ran");
        return;
    }
    forget_other_runs(key, &marker);
}

fn marker_path(key: &str) -> Option<PathBuf> {
    Some(lock_root()?.join(format!("{key}.{}.done", run_id())))
}

/// What identifies this test run.
///
/// `NEXTEST_RUN_ID`, filtered to file-name characters because it becomes a path.
pub(crate) fn run_id() -> String {
    std::env::var("NEXTEST_RUN_ID")
        .ok()
        .map(|id| {
            id.chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .take(40)
                .collect::<String>()
        })
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| std::process::id().to_string())
}

/// How old another run's marker must be before it is tidied away.
///
/// Never a live run's marker: deleting one makes the runs repeat each other's rites.
const STALE_MARKER: Duration = Duration::from_secs(24 * 60 * 60);

/// Removes the markers earlier runs left for `key` once they are older than [`STALE_MARKER`].
fn forget_other_runs(key: &str, keep: &std::path::Path) {
    let Some(root) = lock_root() else { return };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let prefix = format!("{key}.");
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > STALE_MARKER);
        if stale && name.starts_with(&prefix) && name.ends_with(".done") {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A key nothing else uses, so these tests cannot reclaim each other's markers.
    fn clear(key: &str) {
        if let Some(marker) = marker_path(key) {
            let _ = std::fs::remove_file(marker);
        }
    }

    #[tokio::test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn a_rite_runs_once_and_then_never_again_in_this_run() {
        // The second caller in the same run does not repeat the work.
        let key = "unit-once-first";
        clear(key);
        let ran = AtomicUsize::new(0);
        for _ in 0..5 {
            once_per_run(key, || async {
                ran.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        }
        assert_eq!(
            ran.load(Ordering::SeqCst),
            1,
            "the rite must run exactly once"
        );
        clear(key);
    }

    #[tokio::test]
    async fn a_new_run_performs_the_rite_again() {
        let key = "unit-once-newrun";
        clear(key);
        let ran = AtomicUsize::new(0);
        once_per_run(key, || async {
            ran.fetch_add(1, Ordering::SeqCst);
        })
        .await;

        clear(key);
        once_per_run(key, || async {
            ran.fetch_add(1, Ordering::SeqCst);
        })
        .await;
        assert_eq!(ran.load(Ordering::SeqCst), 2);
        clear(key);
    }

    #[tokio::test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn the_promise_is_recorded_only_after_the_rite_has_finished() {
        // Asserted from inside the rite: no marker exists until it finishes.
        let key = "unit-once-ordering";
        clear(key);
        let marker = marker_path(key).expect("a marker path");
        let seen_during = AtomicUsize::new(0);
        once_per_run(key, || async {
            if marker.exists() {
                seen_during.fetch_add(1, Ordering::SeqCst);
            }
        })
        .await;

        assert_eq!(
            seen_during.load(Ordering::SeqCst),
            0,
            "the marker existed while the rite was still running, so an interrupted rite \
             would be recorded as done"
        );
        assert!(marker.exists(), "a finished rite must be recorded");
        clear(key);
    }

    #[tokio::test]
    async fn the_markers_of_earlier_runs_are_forgotten() {
        let key = "unit-once-tidy";
        clear(key);
        let root = lock_root().expect("a lock root");
        std::fs::create_dir_all(&root).expect("the lock directory");
        let stale = root.join(format!("{key}.an-earlier-run.done"));
        std::fs::write(&stale, "1").expect("write a stale marker");
        let two_days_ago = std::time::SystemTime::now() - 2 * STALE_MARKER;
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .and_then(|file| file.set_modified(two_days_ago))
            .expect("back-date the marker");

        once_per_run(key, || async {}).await;

        assert!(
            !stale.exists(),
            "a stale marker from another run must be removed"
        );
        clear(key);
    }

    #[tokio::test]
    async fn a_concurrent_runs_marker_is_left_alone() {
        let key = "unit-once-concurrent";
        clear(key);
        let root = lock_root().expect("a lock root");
        std::fs::create_dir_all(&root).expect("the lock directory");
        let concurrent = root.join(format!("{key}.a-concurrent-run.done"));
        std::fs::write(&concurrent, "1").expect("write a concurrent run's marker");

        once_per_run(key, || async {}).await;

        assert!(
            concurrent.exists(),
            "a marker another run wrote moments ago must survive this run's rite"
        );
        let _ = std::fs::remove_file(concurrent);
        clear(key);
    }

    #[test]
    fn the_run_id_is_safe_to_put_in_a_path() {
        let id = run_id();
        assert!(!id.is_empty());
        assert!(
            id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "{id} would escape the marker directory"
        );
    }
}
