//! [`BundleFingerprint`] — which bundle steps a rebuild can honestly skip.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, instrument};

use super::dist_snapshot::all_present;
use super::step_print::StepPrint;
use super::step_record::StepRecord;
use crate::build_error::BuildError;

const RECORD: &str = ".fundamental-bundle";

/// What each of a browser bundle's steps last saw, fingerprinted separately per step.
///
/// A step is current only when its inputs match and every output it recorded is still
/// present. The css print covers the named roots and the command string, which is a
/// heuristic; `--force` exists for when it is wrong.
#[derive(Debug, Default)]
pub struct BundleFingerprint {
    /// Step name to what it last saw and what it left behind.
    steps: BTreeMap<String, StepRecord>,
    path: PathBuf,
}

impl BundleFingerprint {
    /// Read the record from `dist`, or an empty one (every step stale) if absent or unreadable.
    #[must_use]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn load(dist: &Path) -> Self {
        let path = dist.join(RECORD);
        let mut steps = BTreeMap::new();
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines() {
                if let Some((step, rest)) = line.split_once('\t')
                    && let Some(record) = StepRecord::parse(rest)
                {
                    steps.insert(step.to_string(), record);
                }
            }
        }
        Self { steps, path }
    }

    /// Whether `step` can be skipped: its print is non-empty and unchanged, and every
    /// output it recorded is still in `dist`.
    #[must_use]
    pub fn is_current(&self, step: &str, print: &StepPrint, dist: &Path) -> bool {
        if print.is_empty() {
            return false;
        }
        let Some(record) = self.steps.get(step) else {
            return false;
        };
        // A record naming no outputs never supports a skip.
        !record.outputs().is_empty()
            && record.print() == print
            && all_present(dist, record.outputs())
    }

    /// What `step` recorded writing last time, for a skipped step to carry forward.
    #[must_use]
    pub fn outputs_of(&self, step: &str) -> Vec<String> {
        self.steps
            .get(step)
            .map(|record| record.outputs().to_vec())
            .unwrap_or_default()
    }

    /// Record what `step` has now seen and what it wrote, after its artefacts are on disk.
    pub fn set(&mut self, step: &str, print: StepPrint, outputs: Vec<String>) {
        self.steps
            .insert(step.to_string(), StepRecord::new(print, outputs));
    }

    /// Delete the record, so an interrupted build leaves none claiming half-written artefacts.
    ///
    /// # Errors
    /// Returns [`BuildError::Io`] if the file exists and cannot be removed.
    #[instrument(skip_all, fields(path = %dist.display()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn invalidate(dist: &Path) -> Result<(), BuildError> {
        let path = dist.join(RECORD);
        match fs::remove_file(&path) {
            Ok(()) => {
                debug!("bundle fingerprint invalidated");
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(BuildError::Io { path, source }),
        }
    }

    /// Write the record out, always after the artefacts, so a failed build never looks complete.
    ///
    /// # Errors
    /// Returns [`BuildError::Io`] if the directory or file cannot be written.
    #[instrument(skip(self), fields(path = %self.path.display()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn save(&self) -> Result<(), BuildError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| BuildError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let body: String = self
            .steps
            .iter()
            .map(|(step, record)| format!("{step}\t{record}\n"))
            .collect();
        fs::write(&self.path, body).map_err(|source| BuildError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let src = tmp.path().join("styles");
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&src).expect("src");
        fs::create_dir_all(&dist).expect("dist");
        fs::write(src.join("input.css"), "a{}").expect("css");
        fs::write(dist.join("app.css"), "a{}").expect("built css");
        (tmp, src, dist)
    }

    fn built() -> Vec<String> {
        vec!["app.css".to_string()]
    }

    #[test]
    fn a_missing_record_makes_every_step_stale() {
        let (_t, src, dist) = fixture();
        let print = StepPrint::of_paths(&[src.as_path()], "tailwindcss");
        assert!(!BundleFingerprint::load(&dist).is_current("css", &print, &dist));
    }

    #[test]
    fn an_unchanged_step_is_current_after_a_save() {
        let (_t, src, dist) = fixture();
        let print = StepPrint::of_paths(&[src.as_path()], "tailwindcss");

        let mut fp = BundleFingerprint::load(&dist);
        fp.set("css", print.clone(), built());
        fp.save().expect("saved");

        assert!(BundleFingerprint::load(&dist).is_current("css", &print, &dist));
    }

    #[test]
    fn an_edited_input_is_no_longer_current() {
        let (_t, src, dist) = fixture();
        let mut fp = BundleFingerprint::load(&dist);
        fp.set(
            "css",
            StepPrint::of_paths(&[src.as_path()], "tailwindcss"),
            built(),
        );
        fp.save().expect("saved");

        fs::write(src.join("input.css"), "a{color:red}").expect("edit");
        let now = StepPrint::of_paths(&[src.as_path()], "tailwindcss");
        assert!(!BundleFingerprint::load(&dist).is_current("css", &now, &dist));
    }

    /// A deleted artefact with unchanged inputs is rebuilt.
    #[test]
    fn a_deleted_output_makes_the_step_stale_even_though_its_inputs_are_untouched() {
        let (_t, src, dist) = fixture();
        let print = StepPrint::of_paths(&[src.as_path()], "tailwindcss");
        let mut fp = BundleFingerprint::load(&dist);
        fp.set("css", print.clone(), built());
        fp.save().expect("saved");
        assert!(BundleFingerprint::load(&dist).is_current("css", &print, &dist));

        fs::remove_file(dist.join("app.css")).expect("delete the built stylesheet");

        assert!(
            !BundleFingerprint::load(&dist).is_current("css", &print, &dist),
            "a step whose output is gone must re-run, whatever its inputs say"
        );
    }

    #[test]
    fn the_steps_are_independent() {
        let (_t, src, dist) = fixture();
        let wasm_src = src.parent().expect("parent").join("wasm-src");
        fs::create_dir_all(&wasm_src).expect("wasm src");
        fs::write(wasm_src.join("lib.rs"), "fn a() {}").expect("rs");
        fs::write(dist.join("app.js"), "//").expect("built js");

        let css = StepPrint::of_paths(&[src.as_path()], "tailwindcss");
        let wasm = StepPrint::of_paths(&[wasm_src.as_path()], "hydrate release");
        let mut fp = BundleFingerprint::load(&dist);
        fp.set("css", css, built());
        fp.set("wasm", wasm.clone(), vec!["app.js".to_string()]);
        fp.save().expect("saved");

        fs::write(src.join("input.css"), "a{color:red}").expect("edit css only");
        let css_now = StepPrint::of_paths(&[src.as_path()], "tailwindcss");
        let loaded = BundleFingerprint::load(&dist);
        assert!(
            !loaded.is_current("css", &css_now, &dist),
            "css must be stale"
        );
        assert!(
            loaded.is_current("wasm", &wasm, &dist),
            "wasm must stay current"
        );
    }

    /// One step's missing output does not make another step stale.
    #[test]
    fn a_missing_output_makes_only_its_own_step_stale() {
        let (_t, src, dist) = fixture();
        fs::write(dist.join("app.js"), "//").expect("built js");
        let css = StepPrint::of_paths(&[src.as_path()], "tailwindcss");
        let wasm = StepPrint::of_paths(&[src.as_path()], "hydrate release");
        let mut fp = BundleFingerprint::load(&dist);
        fp.set("css", css.clone(), built());
        fp.set("wasm", wasm.clone(), vec!["app.js".to_string()]);
        fp.save().expect("saved");

        fs::remove_file(dist.join("app.css")).expect("delete only the stylesheet");

        let loaded = BundleFingerprint::load(&dist);
        assert!(!loaded.is_current("css", &css, &dist), "css must be stale");
        assert!(
            loaded.is_current("wasm", &wasm, &dist),
            "wasm must not be dragged down with it"
        );
    }

    #[test]
    fn an_empty_print_is_never_current_even_if_recorded() {
        let (_t, _src, dist) = fixture();
        let empty = StepPrint::of_paths(&[Path::new("/no/such/dir")], "assets");
        let mut fp = BundleFingerprint::load(&dist);
        fp.set("assets", empty.clone(), built());
        fp.save().expect("saved");
        assert!(!BundleFingerprint::load(&dist).is_current("assets", &empty, &dist));
    }

    #[test]
    fn a_record_with_no_outputs_is_never_current() {
        let (_t, src, dist) = fixture();
        let print = StepPrint::of_paths(&[src.as_path()], "tailwindcss");
        let mut fp = BundleFingerprint::load(&dist);
        fp.set("css", print.clone(), Vec::new());
        fp.save().expect("saved");
        assert!(!BundleFingerprint::load(&dist).is_current("css", &print, &dist));
    }

    #[test]
    fn a_skipped_step_can_carry_its_outputs_forward() {
        let (_t, src, dist) = fixture();
        let print = StepPrint::of_paths(&[src.as_path()], "tailwindcss");
        let mut fp = BundleFingerprint::load(&dist);
        fp.set("css", print, built());
        fp.save().expect("saved");

        let loaded = BundleFingerprint::load(&dist);
        assert_eq!(loaded.outputs_of("css"), built());
        assert!(loaded.outputs_of("never-ran").is_empty());
    }

    #[test]
    fn invalidate_removes_the_record_and_tolerates_its_absence() {
        let (_t, src, dist) = fixture();
        let mut fp = BundleFingerprint::load(&dist);
        fp.set("css", StepPrint::of_paths(&[src.as_path()], "x"), built());
        fp.save().expect("saved");
        assert!(dist.join(RECORD).exists());

        BundleFingerprint::invalidate(&dist).expect("removed");
        assert!(!dist.join(RECORD).exists());
        BundleFingerprint::invalidate(&dist).expect("absent is fine");
    }

    #[test]
    fn a_corrupt_record_makes_steps_stale_rather_than_erroring() {
        let (_t, src, dist) = fixture();
        fs::create_dir_all(&dist).expect("dist");
        fs::write(dist.join(RECORD), "css\tnot-a-print\nnonsense\n").expect("corrupt");
        let print = StepPrint::of_paths(&[src.as_path()], "tailwindcss");
        assert!(!BundleFingerprint::load(&dist).is_current("css", &print, &dist));
    }
}
