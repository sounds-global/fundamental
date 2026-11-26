//! [`PagesBuild`], what a web app's `build.rs` runs: scan, check, generate, declare.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fundamental_core::Error as _;
use tracing::{error, info, instrument};

use crate::architecture::ArchitectureCheck;
use crate::build_error::BuildError;
use crate::hydration::Hydration;
use crate::route::{
    RouteTree, islands_in_pages, render_address_registration, render_hydrate_entry,
    render_link_builders, render_page_registration,
};
use crate::wasm::IslandsCrate;

/// What a web app's `build.rs` runs, in one call: scan `pages/`, check it, and write
/// `pages.rs`, `hydrate.rs`, `addresses.rs` and `routes.rs` into `OUT_DIR`.
///
///
/// ```no_run
/// // build.rs
/// fn main() -> std::process::ExitCode {
///     fundamental_build::PagesBuild::run_build_script(fundamental_build::Hydration::Plain)
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct PagesBuild {
    app_dir: PathBuf,
    out_dir: PathBuf,
    hydration: Hydration,
}

impl PagesBuild {
    /// A build of the app in `app_dir` writing into `out_dir`.
    pub fn new(app_dir: impl Into<PathBuf>, out_dir: impl Into<PathBuf>) -> Self {
        Self {
            app_dir: app_dir.into(),
            out_dir: out_dir.into(),
            hydration: Hydration::Plain,
        }
    }

    /// The build cargo is running: the package's directory and its `OUT_DIR`.
    ///
    /// # Errors
    ///
    /// [`BuildError::NotInBuildScript`] when cargo did not set `CARGO_MANIFEST_DIR` or `OUT_DIR`.
    pub fn from_env() -> Result<Self, BuildError> {
        Ok(Self::new(
            cargo_variable("CARGO_MANIFEST_DIR")?,
            cargo_variable("OUT_DIR")?,
        ))
    }

    /// What the browser build hydrates, and how it starts.
    pub fn hydration(mut self, hydration: Hydration) -> Self {
        self.hydration = hydration;
        self
    }

    /// Run the build inside a `build.rs` whose package is the app, reporting a refusal to cargo.
    pub fn run_build_script(hydration: Hydration) -> ExitCode {
        Self::run_build_script_in(".", hydration)
    }

    /// Like [`run_build_script`](Self::run_build_script), for a package beside the app's pages;
    /// `app` is the app's directory relative to the package's.
    pub fn run_build_script_in(app: impl AsRef<Path>, hydration: Hydration) -> ExitCode {
        let build = Self::from_env().map(|build| Self {
            app_dir: build.app_dir.join(app.as_ref()),
            ..build.hydration(hydration)
        });
        match build.and_then(|build| build.run()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(refusal) => {
                error!(code = refusal.error_code(), error = %refusal, "the pages build refused");
                eprintln!("error[{}]: {refusal}", refusal.error_code());
                ExitCode::FAILURE
            }
        }
    }

    /// Scan, check, generate and declare the inputs.
    ///
    /// # Errors
    ///
    /// Any [`BuildError`] the scan raises; [`BuildError::ArchitectureRefused`] with every
    /// finding; [`BuildError::Io`] when a file cannot be read or written.
    #[instrument(skip_all, fields(app_dir = %self.app_dir.display()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn run(&self) -> Result<(), BuildError> {
        let pages_dir = self.app_dir.join("pages");
        rerun_if_changed(&pages_dir);
        let tree = RouteTree::scan(&pages_dir)?;
        let pages_dir = fs::canonicalize(&pages_dir).map_err(|e| BuildError::io(&pages_dir, e))?;
        let app_dir =
            fs::canonicalize(&self.app_dir).map_err(|e| BuildError::io(&self.app_dir, e))?;

        let mut inputs = Vec::new();
        let verdict = ArchitectureCheck {
            app_dir: &app_dir,
            pages_dir: &pages_dir,
            tree: &tree,
        }
        .run(&mut inputs);
        for input in &inputs {
            rerun_if_changed(input);
        }
        verdict?;
        let hydration = self.hydration_for(&tree, &pages_dir)?;
        info!(inputs = inputs.len(), "page checks passed");

        self.write("pages.rs", &render_page_registration(&tree, &pages_dir))?;
        self.write(
            "hydrate.rs",
            &render_hydrate_entry(&tree, &pages_dir, hydration),
        )?;
        self.write(
            "addresses.rs",
            &render_address_registration(&tree, &pages_dir),
        )?;
        self.write("routes.rs", &render_link_builders(&tree))?;
        Ok(())
    }

    /// What the app crate's browser entry hydrates: no page when the islands have their own crate.
    fn hydration_for(&self, tree: &RouteTree, pages_dir: &Path) -> Result<Hydration, BuildError> {
        // Declared only when present: cargo reruns a build script every build for a missing path.
        if !IslandsCrate::is_in(&self.app_dir) {
            return Ok(self.hydration);
        }
        rerun_if_changed(&self.app_dir.join("islands").join("Cargo.toml"));
        let dead = islands_in_pages(tree, pages_dir)?;
        if !dead.is_empty() {
            return Err(BuildError::island_in_pages(dead));
        }
        Ok(Hydration::NoPages)
    }

    fn write(&self, name: &str, source: &str) -> Result<(), BuildError> {
        let path = self.out_dir.join(name);
        fs::write(&path, source).map_err(|e| BuildError::io(path, e))
    }
}

pub(crate) fn rerun_if_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}

pub(crate) fn cargo_variable(variable: &str) -> Result<OsString, BuildError> {
    std::env::var_os(variable).ok_or_else(|| BuildError::NotInBuildScript {
        variable: variable.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_build_writes_the_three_sources() {
        let d = TempDir::new().unwrap();
        let app = d.path().join("app");
        fs::create_dir_all(app.join("pages")).unwrap();
        fs::write(app.join("pages/index.rs"), "pub fn page() {}").unwrap();
        fs::write(
            app.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let out = d.path().join("out");
        fs::create_dir_all(&out).unwrap();
        PagesBuild::new(&app, &out)
            .hydration(Hydration::Live)
            .run()
            .unwrap();
        for name in ["pages.rs", "hydrate.rs", "addresses.rs"] {
            assert!(out.join(name).is_file(), "{name}");
        }
        let hydrate = fs::read_to_string(out.join("hydrate.rs")).unwrap();
        assert!(hydrate.contains("live_boot"));
    }

    #[test]
    fn a_build_writes_a_builder_for_every_page() {
        let d = TempDir::new().unwrap();
        let app = d.path().join("app");
        fs::create_dir_all(app.join("pages/portfolio/[slug]")).unwrap();
        fs::write(app.join("pages/index.rs"), "pub fn page() {}").unwrap();
        fs::write(
            app.join("pages/portfolio/[slug]/index.rs"),
            "pub fn page() {}",
        )
        .unwrap();
        fs::write(
            app.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let out = d.path().join("out");
        fs::create_dir_all(&out).unwrap();
        PagesBuild::new(&app, &out).run().unwrap();
        let routes = fs::read_to_string(out.join("routes.rs")).unwrap();
        assert!(routes.contains("pub mod routes"), "{routes}");
        assert!(routes.contains("pub const fn index()"), "{routes}");
        assert!(routes.contains("pub fn portfolio_slug("), "{routes}");
    }

    fn app_with_islands_crate(d: &TempDir, page: &str) -> (PathBuf, PathBuf) {
        let app = d.path().join("app");
        fs::create_dir_all(app.join("pages")).unwrap();
        fs::create_dir_all(app.join("islands")).unwrap();
        fs::write(app.join("pages/index.rs"), page).unwrap();
        fs::write(
            app.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            app.join("islands/Cargo.toml"),
            "[package]\nname = \"app_islands\"\n",
        )
        .unwrap();
        let out = d.path().join("out");
        fs::create_dir_all(&out).unwrap();
        (app, out)
    }

    #[test]
    fn an_island_in_pages_beside_an_islands_crate_is_refused() {
        let d = TempDir::new().unwrap();
        let (app, out) = app_with_islands_crate(&d, "pub fn page() {}\n#[island] fn Counter() {}");
        let err = PagesBuild::new(&app, &out).run().unwrap_err();
        assert_eq!(err.error_code(), "BUILD_040");
        assert!(err.to_string().contains("index.rs"), "{err}");
    }

    #[test]
    fn an_app_with_an_islands_crate_hydrates_no_page() {
        let d = TempDir::new().unwrap();
        let (app, out) = app_with_islands_crate(&d, "pub fn page() {}");
        PagesBuild::new(&app, &out).run().unwrap();
        let hydrate = fs::read_to_string(out.join("hydrate.rs")).unwrap();
        assert!(!hydrate.contains("#[path"), "{hydrate}");
    }

    #[test]
    fn an_app_without_pages_is_an_io_error_naming_them() {
        let d = TempDir::new().unwrap();
        let err = PagesBuild::new(d.path(), d.path()).run().unwrap_err();
        assert_eq!(err.error_code(), "BUILD_001");
        assert!(err.to_string().contains("pages"));
    }

    #[test]
    fn a_variable_cargo_did_not_set_is_named() {
        let err = cargo_variable("FUNDAMENTAL_BUILD_NEVER_SET").unwrap_err();
        assert_eq!(err.error_code(), "BUILD_037");
        assert!(err.to_string().contains("FUNDAMENTAL_BUILD_NEVER_SET"));
    }
}
