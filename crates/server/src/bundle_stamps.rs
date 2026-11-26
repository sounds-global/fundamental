use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// What a change to the files a page loads asks of the open page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BundleChange {
    /// Only stylesheets changed: the page swaps them in place.
    Css,
    /// A script or the wasm changed, or a file came or went: the page reloads.
    Reload,
}

/// The length and modification time of every stylesheet, script and wasm file under a set of roots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BundleStamps {
    files: BTreeMap<PathBuf, (u64, SystemTime)>,
}

impl BundleStamps {
    /// The stamps under `roots` now; an unreadable entry is left out.
    pub(crate) fn take(roots: &[PathBuf]) -> Self {
        let mut files = BTreeMap::new();
        for root in roots {
            collect(root, &mut files);
        }
        Self { files }
    }

    /// What going from `self` to `next` asks of an open page, or `None` when nothing a page loads changed.
    pub(crate) fn change(&self, next: &Self) -> Option<BundleChange> {
        let moved = self
            .files
            .keys()
            .chain(next.files.keys())
            .filter(|path| self.files.get(*path) != next.files.get(*path));
        let mut change = None;
        for path in moved {
            if !is_stylesheet(path)
                || !self.files.contains_key(path)
                || !next.files.contains_key(path)
            {
                return Some(BundleChange::Reload);
            }
            change = Some(BundleChange::Css);
        }
        change
    }

    /// The newest stylesheet's modification time, in nanoseconds.
    pub(crate) fn css_stamp(&self) -> u128 {
        self.files
            .iter()
            .filter(|(path, _)| is_stylesheet(path))
            .filter_map(|(_, (_, modified))| modified.duration_since(UNIX_EPOCH).ok())
            .map(|since| since.as_nanos())
            .max()
            .unwrap_or_default()
    }
}

/// Whether a page loads `path` as a stylesheet, script or wasm module.
fn is_watched(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("css" | "js" | "mjs" | "wasm")
    )
}

fn is_stylesheet(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("css")
}

fn collect(dir: &Path, files: &mut BTreeMap<PathBuf, (u64, SystemTime)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect(&path, files);
        } else if is_watched(&path)
            && let Ok(modified) = meta.modified()
        {
            files.insert(path, (meta.len(), modified));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dist() -> (tempfile::TempDir, Vec<PathBuf>) {
        let dir = tempfile::tempdir().expect("a dist");
        fs::write(dir.path().join("app.css"), "a{}").expect("css");
        fs::write(dir.path().join("www.js"), "export{}").expect("js");
        let roots = vec![dir.path().to_path_buf()];
        (dir, roots)
    }

    #[test]
    fn an_untouched_bundle_has_no_change() {
        let (_dir, roots) = dist();
        assert_eq!(
            BundleStamps::take(&roots).change(&BundleStamps::take(&roots)),
            None
        );
    }

    #[test]
    fn a_stylesheet_edit_is_a_css_change() {
        let (dir, roots) = dist();
        let before = BundleStamps::take(&roots);
        fs::write(dir.path().join("app.css"), "a{color:red}").expect("an edit");
        assert_eq!(
            before.change(&BundleStamps::take(&roots)),
            Some(BundleChange::Css)
        );
    }

    #[test]
    fn a_wasm_edit_is_a_reload() {
        let (dir, roots) = dist();
        let before = BundleStamps::take(&roots);
        fs::write(dir.path().join("www_bg.wasm"), b"\0asm").expect("a new module");
        assert_eq!(
            before.change(&BundleStamps::take(&roots)),
            Some(BundleChange::Reload)
        );
    }

    #[test]
    fn a_record_no_page_loads_is_no_change() {
        let (dir, roots) = dist();
        let before = BundleStamps::take(&roots);
        fs::write(dir.path().join(".fingerprint.json"), "{}").expect("a record");
        assert_eq!(before.change(&BundleStamps::take(&roots)), None);
    }

    #[test]
    fn the_css_stamp_moves_with_a_stylesheet() {
        let (dir, roots) = dist();
        let before = BundleStamps::take(&roots).css_stamp();
        std::thread::sleep(std::time::Duration::from_millis(5));
        fs::write(dir.path().join("app.css"), "a{color:red}").expect("an edit");
        assert_ne!(BundleStamps::take(&roots).css_stamp(), before);
    }
}
