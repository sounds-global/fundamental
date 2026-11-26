//! [`RoutesBuild`], what a routes crate's `build.rs` runs: an app's pages, named for every crate.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fundamental_core::Error as _;
use tracing::{error, instrument};

use crate::build_error::BuildError;
use crate::pages_build::{cargo_variable, rerun_if_changed};
use crate::route::{RouteTree, render_link_builders, render_page_slugs};

/// What a routes crate's `build.rs` runs: `routes.rs` with the builder of every page an app
/// serves and `page_slugs()`, for the domain crates that may not depend on the app.
///
/// ```no_run
/// // routes/build.rs, beside the app's `pages/` one directory up
/// fn main() -> std::process::ExitCode {
///     fundamental_build::RoutesBuild::run_build_script("..")
/// }
/// ```
/// ```ignore
/// // routes/src/lib.rs
/// include!(concat!(env!("OUT_DIR"), "/routes.rs"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct RoutesBuild {
    app_dir: PathBuf,
    out_dir: PathBuf,
}

impl RoutesBuild {
    /// A build of the routes of the app in `app_dir`, writing into `out_dir`.
    pub fn new(app_dir: impl Into<PathBuf>, out_dir: impl Into<PathBuf>) -> Self {
        Self {
            app_dir: app_dir.into(),
            out_dir: out_dir.into(),
        }
    }

    /// Run the build inside a routes crate's `build.rs`; `app` is the app's directory relative
    /// to the routes crate's own.
    pub fn run_build_script(app: impl AsRef<Path>) -> ExitCode {
        let build = cargo_variable("CARGO_MANIFEST_DIR").and_then(|manifest| {
            Ok(Self::new(
                Path::new(&manifest).join(app.as_ref()),
                cargo_variable("OUT_DIR")?,
            ))
        });
        match build.and_then(|build| build.run()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(refusal) => {
                error!(code = refusal.error_code(), error = %refusal, "the routes build refused");
                eprintln!("error[{}]: {refusal}", refusal.error_code());
                ExitCode::FAILURE
            }
        }
    }

    /// Scan the app's pages and write `routes.rs`.
    ///
    /// # Errors
    ///
    /// Any [`BuildError`] the scan raises; [`BuildError::Io`] when the file cannot be written.
    #[instrument(skip_all, fields(app_dir = %self.app_dir.display()))]
    pub fn run(&self) -> Result<(), BuildError> {
        let tree = self.scan()?;
        let mut source = render_link_builders(&tree);
        source.push_str(&render_page_slugs(&tree));
        let path = self.out_dir.join("routes.rs");
        fs::write(&path, source).map_err(|e| BuildError::io(path, e))
    }

    fn scan(&self) -> Result<RouteTree, BuildError> {
        let pages_dir = self.app_dir.join("pages");
        rerun_if_changed(&pages_dir);
        let pages_dir = fs::canonicalize(&pages_dir).map_err(|e| BuildError::io(&pages_dir, e))?;
        RouteTree::scan(&pages_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_routes_crate_gets_a_builder_for_every_page_and_their_slugs() {
        let d = TempDir::new().unwrap();
        let pages = d.path().join("app/pages");
        fs::create_dir_all(pages.join("mission")).unwrap();
        fs::write(pages.join("index.rs"), "pub fn page() {}").unwrap();
        fs::write(
            pages.join("mission/index.rs"),
            "slugs! { nl = \"missie\" }\npub fn page() {}",
        )
        .unwrap();
        let out = d.path().join("out");
        fs::create_dir_all(&out).unwrap();
        RoutesBuild::new(d.path().join("app"), &out).run().unwrap();
        let routes = fs::read_to_string(out.join("routes.rs")).unwrap();
        assert!(routes.contains("pub mod routes"), "{routes}");
        assert!(routes.contains("pub const fn mission()"), "{routes}");
        assert!(routes.contains("pub fn page_slugs()"), "{routes}");
        assert!(routes.contains("nl = \"missie\""), "{routes}");
    }

    #[test]
    fn an_app_without_pages_is_an_io_error_naming_them() {
        let d = TempDir::new().unwrap();
        let err = RoutesBuild::new(d.path(), d.path()).run().unwrap_err();
        assert_eq!(err.error_code(), "BUILD_001");
        assert!(err.to_string().contains("pages"));
    }
}
