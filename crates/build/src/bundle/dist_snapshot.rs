//! [`DistSnapshot`] — what the bundle directory held before a step ran.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// A file's length and modification time in nanoseconds.
///
/// Tools rewrite byte-identical files within one second, so a seconds stamp would
/// record no outputs and the step would re-run on every build.
type Stamp = (u64, u128);

/// The state of a bundle directory at a moment, so the files a step wrote can be named
/// afterwards without every step declaring them.
#[derive(Debug, Default)]
pub struct DistSnapshot {
    /// Path relative to the bundle directory to its [`Stamp`].
    entries: BTreeMap<String, Stamp>,
}

impl DistSnapshot {
    /// Record what `dist` holds now; a missing directory snapshots empty.
    #[must_use]
    pub fn capture(dist: &Path) -> Self {
        let mut snapshot = Self::default();
        snapshot.absorb(dist, dist);
        snapshot
    }

    /// The paths under `dist` that appeared or changed (length or mtime) since this
    /// snapshot, relative to `dist`.
    #[must_use]
    pub fn written_since(&self, dist: &Path) -> Vec<String> {
        let now = Self::capture(dist);
        now.entries
            .into_iter()
            .filter(|(path, stamp)| self.entries.get(path) != Some(stamp))
            .map(|(path, _)| path)
            .collect()
    }

    fn absorb(&mut self, root: &Path, path: &Path) {
        let Ok(meta) = fs::metadata(path) else {
            return;
        };
        if meta.is_dir() {
            let Ok(entries) = fs::read_dir(path) else {
                return;
            };
            for entry in entries.flatten() {
                self.absorb(root, &entry.path());
            }
            return;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            return;
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_nanos());
        self.entries.insert(
            relative.to_string_lossy().into_owned(),
            (meta.len(), modified),
        );
    }
}

/// Whether every one of `outputs` still exists under `dist`.
#[must_use]
pub fn all_present(dist: &Path, outputs: &[String]) -> bool {
    outputs
        .iter()
        .all(|relative| PathBuf::from(dist).join(relative).exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn dist() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&dist).expect("dist");
        (tmp, dist)
    }

    #[test]
    fn a_missing_directory_snapshots_empty() {
        let tmp = TempDir::new().expect("tempdir");
        let absent = tmp.path().join("nope");
        assert!(DistSnapshot::capture(&absent).entries.is_empty());
    }

    #[test]
    fn a_new_file_is_reported_as_written() {
        let (_t, dist) = dist();
        let before = DistSnapshot::capture(&dist);
        fs::write(dist.join("app.css"), "a{}").expect("write");
        assert_eq!(before.written_since(&dist), vec!["app.css".to_string()]);
    }

    #[test]
    fn an_untouched_file_is_not_reported() {
        let (_t, dist) = dist();
        fs::write(dist.join("app.css"), "a{}").expect("write");
        let before = DistSnapshot::capture(&dist);
        assert!(before.written_since(&dist).is_empty());
    }

    #[test]
    fn a_rewritten_file_is_reported_even_within_one_timestamp_tick() {
        let (_t, dist) = dist();
        fs::write(dist.join("app.css"), "a{}").expect("write");
        let before = DistSnapshot::capture(&dist);
        // Same tick, different length — which is the case a timestamp alone misses.
        fs::write(dist.join("app.css"), "a{color:red}").expect("rewrite");
        assert_eq!(before.written_since(&dist), vec!["app.css".to_string()]);
    }

    /// A byte-identical rewrite within one second is still recorded as an output.
    #[test]
    fn an_identical_rewrite_in_the_same_second_is_still_a_write() {
        let (_t, dist) = dist();
        fs::write(dist.join("app.css"), "a{}").expect("write");
        let before = DistSnapshot::capture(&dist);
        fs::write(dist.join("app.css"), "a{}").expect("rewrite the same bytes");
        assert_eq!(
            before.written_since(&dist),
            vec!["app.css".to_string()],
            "a same-second, same-length rewrite must still count as written"
        );
    }

    #[test]
    fn nested_outputs_are_recorded_under_their_relative_path() {
        let (_t, dist) = dist();
        let before = DistSnapshot::capture(&dist);
        fs::create_dir_all(dist.join("assets/fonts")).expect("dirs");
        fs::write(dist.join("assets/fonts/a.woff2"), "x").expect("write");
        assert_eq!(
            before.written_since(&dist),
            vec!["assets/fonts/a.woff2".to_string()]
        );
    }

    #[test]
    fn presence_is_all_or_nothing() {
        let (_t, dist) = dist();
        fs::write(dist.join("app.css"), "a{}").expect("write");
        assert!(all_present(&dist, &["app.css".to_string()]));
        assert!(!all_present(
            &dist,
            &["app.css".to_string(), "gone.js".to_string()]
        ));
    }

    #[test]
    fn nothing_recorded_is_vacuously_present() {
        let (_t, dist) = dist();
        assert!(all_present(&dist, &[]));
    }
}
