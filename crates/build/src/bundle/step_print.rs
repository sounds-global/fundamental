//! [`StepPrint`] — a comparable fingerprint of one bundle step's inputs.

use std::fmt::{self, Display};
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// What one bundle step's inputs looked like, in a form two builds can compare.
///
/// Values rather than a hash, so the record file shows why a step re-ran; the length
/// catches a rewrite within one coarse mtime tick.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepPrint {
    files: u64,
    bytes: u64,
    newest: u64,
    salt: String,
}

impl StepPrint {
    /// Fingerprint every file under `roots`, plus `salt`; a missing root contributes nothing.
    #[must_use]
    pub fn of_paths(roots: &[&Path], salt: impl Into<String>) -> Self {
        let mut print = Self {
            salt: salt.into(),
            ..Self::default()
        };
        for root in roots {
            print.absorb(root);
        }
        print
    }

    /// Fingerprint the source set cargo recorded in a `.d` dep-info file, plus `salt`.
    ///
    /// A missing or unreadable file yields an empty print, which counts as stale.
    #[must_use]
    pub fn of_dep_info(dep_info: &Path, salt: impl Into<String>) -> Self {
        let salt = salt.into();
        let Ok(text) = fs::read_to_string(dep_info) else {
            return Self {
                salt,
                ..Self::default()
            };
        };
        let mut print = Self {
            salt,
            ..Self::default()
        };
        for line in text.lines() {
            // Cargo escapes a space in a path as `\ `.
            let Some((_, deps)) = line.split_once(": ") else {
                continue;
            };
            for dep in split_dep_paths(deps) {
                print.absorb(Path::new(&dep));
            }
        }
        print
    }

    /// Whether this step has no inputs at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files == 0
    }

    fn absorb(&mut self, path: &Path) {
        let Ok(meta) = fs::metadata(path) else {
            return;
        };
        if meta.is_dir() {
            let Ok(entries) = fs::read_dir(path) else {
                return;
            };
            for entry in entries.flatten() {
                self.absorb(&entry.path());
            }
            return;
        }
        self.files += 1;
        self.bytes += meta.len();
        let secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        self.newest = self.newest.max(secs);
    }
}

/// Split a dep-info dependency list on unescaped spaces.
fn split_dep_paths(deps: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = deps.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // `\` before anything but a space is a literal backslash (Windows paths).
            '\\' if chars.peek() == Some(&' ') => {
                chars.next();
                current.push(' ');
            }
            ' ' => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

impl Display for StepPrint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "files={} bytes={} newest={} salt={}",
            self.files, self.bytes, self.newest, self.salt
        )
    }
}

/// An unparseable line yields `None`, which the caller treats as stale.
impl StepPrint {
    /// Read a print back from its recorded line.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        let mut print = Self::default();
        // `salt` is last and may contain spaces and `=`, so it takes the remainder.
        let rest = line.strip_prefix("files=")?;
        let (files, rest) = rest.split_once(" bytes=")?;
        let (bytes, rest) = rest.split_once(" newest=")?;
        let (newest, salt) = rest.split_once(" salt=")?;
        print.files = files.parse().ok()?;
        print.bytes = bytes.parse().ok()?;
        print.newest = newest.parse().ok()?;
        print.salt = salt.to_string();
        Some(print)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn an_absent_root_prints_empty_and_compares_equal_to_itself() {
        let a = StepPrint::of_paths(&[Path::new("/no/such/dir")], "css");
        let b = StepPrint::of_paths(&[Path::new("/no/such/dir")], "css");
        assert!(a.is_empty());
        assert_eq!(a, b);
    }

    #[test]
    fn a_content_change_changes_the_print() {
        let tmp = TempDir::new().expect("tempdir");
        let file = tmp.path().join("input.css");
        fs::write(&file, "a{}").expect("write");
        let before = StepPrint::of_paths(&[tmp.path()], "tailwindcss");
        fs::write(&file, "a{color:red}").expect("rewrite");
        let after = StepPrint::of_paths(&[tmp.path()], "tailwindcss");
        assert_ne!(before, after);
    }

    /// The salt makes "the same files, a different command" stale.
    #[test]
    fn the_salt_participates() {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("a"), "x").expect("write");
        let a = StepPrint::of_paths(&[tmp.path()], "tailwindcss -o app.css");
        let b = StepPrint::of_paths(&[tmp.path()], "tailwindcss -o other.css");
        assert_ne!(a, b);
    }

    #[test]
    fn a_print_round_trips_through_its_line() {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("a"), "xyz").expect("write");
        let print = StepPrint::of_paths(&[tmp.path()], "hydrate release 0.2.126");
        let parsed = StepPrint::parse(&print.to_string()).expect("parses");
        assert_eq!(print, parsed);
    }

    #[test]
    fn an_unparseable_line_is_none_so_the_step_is_stale() {
        assert!(StepPrint::parse("garbage").is_none());
        assert!(StepPrint::parse("files=1 bytes=2").is_none());
    }

    #[test]
    fn dep_info_lists_the_source_set() {
        let tmp = TempDir::new().expect("tempdir");
        let a = tmp.path().join("a.rs");
        let b = tmp.path().join("b.rs");
        fs::write(&a, "fn a() {}").expect("a");
        fs::write(&b, "fn b() {}").expect("b");
        let d = tmp.path().join("wallet.d");
        fs::write(
            &d,
            format!(
                "{}: {} {}\n",
                tmp.path().join("wallet.wasm").display(),
                a.display(),
                b.display()
            ),
        )
        .expect("dep-info");

        let print = StepPrint::of_dep_info(&d, "hydrate");
        assert_eq!(print.files, 2, "{print}");

        fs::write(&a, "fn a() { /* edited */ }").expect("edit");
        assert_ne!(print, StepPrint::of_dep_info(&d, "hydrate"));
    }

    /// Before the first build there is no `.d` file, and the step reads as stale.
    #[test]
    fn a_missing_dep_info_is_empty() {
        let print = StepPrint::of_dep_info(Path::new("/no/such/file.d"), "hydrate");
        assert!(print.is_empty());
    }

    #[test]
    fn escaped_spaces_in_dep_paths_stay_one_path() {
        let paths = split_dep_paths(r"/a/b\ c/d.rs /e/f.rs");
        assert_eq!(
            paths,
            vec!["/a/b c/d.rs".to_string(), "/e/f.rs".to_string()]
        );
    }
}
