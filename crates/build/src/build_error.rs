//! [`BuildError`], the error returned by every scan and discovery operation.

use std::path::PathBuf;

use fundamental_core::{AnyError, Error as _};
use fundamental_macros::Error;

use crate::architecture::ArchitectureFinding;

/// An error from scanning the filesystem or discovering build inputs.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BuildError {
    /// A lower-layer Fundamental error with no more specific variant.
    Other(AnyError),

    /// A filesystem read failed while scanning, with the offending path attached.
    #[error(
        code = "BUILD_001",
        message = "failed to read '{}': {source}", path.display(),
        status = 500
    )]
    Io {
        /// The path whose read failed.
        path: PathBuf,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// A page or special `.rs` file did not parse as Rust.
    #[error(
        code = "BUILD_002",
        message = "failed to parse '{}': {message}", file.display(),
        status = 500
    )]
    Parse {
        /// The file that failed to parse, relative to the pages dir.
        file: PathBuf,
        /// The `syn` parse error message.
        message: String,
    },

    /// A reserved file name appeared in the pages tree.
    #[error(
        code = "BUILD_003",
        message = "reserved file '{}' is not allowed: {reason}", file.display(),
        status = 500
    )]
    ReservedFile {
        /// The offending file, relative to the pages dir.
        file: PathBuf,
        /// Why the name is reserved and what to do instead.
        reason: String,
    },

    /// Two files claim the same root special role (`document.rs` or `not_found.rs`).
    #[error(
        code = "BUILD_004",
        message = "duplicate {kind} file: '{}' conflicts with an earlier one", file.display(),
        status = 500
    )]
    DuplicateSpecial {
        /// The role that was declared twice (`document` or `not_found`).
        kind: String,
        /// The second file found for that role, relative to the pages dir.
        file: PathBuf,
    },

    // `BUILD_005` is retired; codes are stable, so the gap stays.
    /// A page's own `page` function is marked `#[island]`.
    #[error(
        code = "BUILD_010",
        message = "'{}': the `page` function cannot be an `#[island]` — render an island component from it instead, e.g. `#[island] fn Widget() {{ … }}` then `pub fn page() {{ view! {{ <Widget/> }} }}`", file.display(),
        status = 500
    )]
    IslandOnPage {
        /// The page file whose `page` function is wrongly an island.
        file: PathBuf,
    },

    /// `not_found.rs` exports a `Data` type, which is not loaded for it.
    #[error(
        code = "BUILD_011",
        message = "'{}': a `not_found.rs` page cannot declare `Data` — routing has already failed by the time it renders, so there is nothing to load it from. Render from what the request carries (`PageContext`), or open an issue if a 404 genuinely needs a store.", file.display(),
        status = 500
    )]
    PageDataOnNotFound {
        /// The `not_found.rs` file that wrongly declares `Data`.
        file: PathBuf,
    },

    /// A directory's segment is claimed by both its `index.rs` and its `layout.rs`.
    #[error(
        code = "BUILD_012",
        message = "'{}' and its index page both declare slugs for the same directory segment — declare them on the index page, which *is* that segment's route", file.display(),
        status = 500
    )]
    SlugsDeclaredTwice {
        /// The `layout.rs` that duplicates its index page's declaration.
        file: PathBuf,
    },

    /// A page inside a route group declares slugs.
    #[error(
        code = "BUILD_013",
        message = "'{}' is inside a route group, which contributes no URL segment — there is nothing for a slug to rename. Move the declaration to a page whose path it actually appears in.", file.display(),
        status = 500
    )]
    SlugsInRouteGroup {
        /// The file inside a route group that declares slugs.
        file: PathBuf,
    },

    /// `error.rs` exports a `Data` type, which is not loaded for it.
    #[error(
        code = "BUILD_014",
        message = "'{}': an `error.rs` page cannot declare `Data` — it answers a request that already failed, often because a store did not answer. Render from `PageContext::shown_error`.", file.display(),
        status = 500
    )]
    PageDataOnErrorPage {
        /// The `error.rs` file that wrongly declares `Data`.
        file: PathBuf,
    },

    /// A `loader.rs` exports `load`, but its directory has no `index.rs` for it to serve.
    #[error(
        code = "BUILD_015",
        message = "'{}': `load` serves its directory's `index.rs`, and this directory has none. Add the page as `index.rs` beside it.", file.display(),
        status = 500
    )]
    LoaderWithoutIndex {
        /// The `loader.rs` with no page to serve.
        file: PathBuf,
    },

    /// A `loader.rs` exports `frame`, but its directory has no `layout.rs` to receive it.
    #[error(
        code = "BUILD_016",
        message = "'{}': `frame` is what this directory's `layout.rs` receives, and this directory has none. Add the layout, or remove `frame`.", file.display(),
        status = 500
    )]
    FrameWithoutLayout {
        /// The `loader.rs` whose `frame` has no layout.
        file: PathBuf,
    },

    /// A `loader.rs` exports neither `load` nor `frame`, so it serves nothing.
    #[error(
        code = "BUILD_017",
        message = "'{}': a `loader.rs` exports `pub async fn load` for its `index.rs`, `pub async fn frame` for its `layout.rs`, or both — this one exports neither.", file.display(),
        status = 500
    )]
    LoaderServesNothing {
        /// The `loader.rs` that serves nothing.
        file: PathBuf,
    },

    /// A page both exports `Data` and has a `loader.rs` beside it.
    #[error(
        code = "BUILD_018",
        message = "'{}': this page has a `loader.rs` and also exports `Data`. Keep the loader and remove `Data` — `page` takes the loader's model as its argument.", file.display(),
        status = 500
    )]
    LoaderBesideData {
        /// The page file that exports `Data`.
        file: PathBuf,
    },

    /// A `gate.rs` below the pages root.
    #[error(
        code = "BUILD_019",
        message = "'{}': `gate.rs` answers every refusal and belongs at the pages root, beside `not_found.rs`.", file.display(),
        status = 500
    )]
    GateNotAtRoot {
        /// The nested `gate.rs`.
        file: PathBuf,
    },

    /// The `gate.rs` page declares `Data`.
    #[error(
        code = "BUILD_020",
        message = "'{}': `gate.rs` cannot declare `Data` — it answers a request that was refused, so it must load nothing. Render from the `Refused` it is given.", file.display(),
        status = 500
    )]
    PageDataOnGate {
        /// The `gate.rs` that declares `Data`.
        file: PathBuf,
    },

    /// A `gate.rs` with no root `error.rs`.
    #[error(
        code = "BUILD_021",
        message = "'{}': a gate needs the root `error.rs` beside it, which answers every failure the gate does not.", file.display(),
        status = 500
    )]
    GateWithoutErrorPage {
        /// The `gate.rs` without an error page.
        file: PathBuf,
    },

    /// A `policy.rs` that exports no `policy` function.
    #[error(
        code = "BUILD_022",
        message = "'{}': `policy.rs` declares `pub fn policy() -> PagePolicy` for every route at or below its directory, and this one has none. A page at that address is `policy/index.rs`.", file.display(),
        status = 500
    )]
    PolicyWithoutFn {
        /// The `policy.rs` without a `policy` function.
        file: PathBuf,
    },

    /// A page file `x.rs` beside a directory `x/`, in a tree with a loader, a policy or a gate.
    #[error(
        code = "BUILD_023",
        message = "'{}': this page sits beside a directory of the same name, so a `policy.rs` in '{}' would not cover it. Move it to that directory's `index.rs`.", file.display(), directory.display(),
        status = 500
    )]
    RouteBesideItsDirectory {
        /// The page file beside its directory.
        file: PathBuf,
        /// The directory of the same name.
        directory: PathBuf,
    },

    /// A page serves an address a `policy.rs` covers, from outside that policy's directory.
    #[error(
        code = "BUILD_026",
        message = "'{}' serves {}, an address '{}' covers, from outside that policy's directory, so the policy would not run for it. Move the page into the policy's directory.", file.display(), address, policy.display(),
        status = 500
    )]
    RouteOutsideItsAddressPolicy {
        /// The page file.
        file: PathBuf,
        /// The address it serves.
        address: String,
        /// The `policy.rs` that covers the address.
        policy: PathBuf,
    },

    /// The root `layout.rs` has a `frame`.
    #[error(
        code = "BUILD_024",
        message = "'{}': the root layout also wraps `not_found.rs`, `error.rs` and `gate.rs`, which must load nothing, so it takes no `frame`. Put the framed layout in a `(group)/` below the root.", file.display(),
        status = 500
    )]
    FrameOnRootLayout {
        /// The root `loader.rs` that exports `frame`.
        file: PathBuf,
    },

    /// A page that exports `Data` sits under a layout with a `frame`.
    #[error(
        code = "BUILD_025",
        message = "'{}': this page exports `Data` under a layout whose frame is loaded by '{}'. A page under a framed layout loads with a `loader.rs` of its own.", file.display(), frame.display(),
        status = 500
    )]
    DataUnderFramedLayout {
        /// The page that exports `Data`.
        file: PathBuf,
        /// The `loader.rs` that loads the frame.
        frame: PathBuf,
    },

    /// A `web` surface is declared but the app has no `pages/` directory.
    #[error(
        code = "BUILD_006",
        message = "a `web` surface is declared but '{}/pages' is missing", app.display(),
        status = 500
    )]
    MissingPages {
        /// The application directory that lacks a `pages/` dir.
        app: PathBuf,
    },

    /// A `pages/` directory exists but no `web` surface is declared.
    #[error(
        code = "BUILD_007",
        message = "'{}/pages' exists but no `web` surface is declared", app.display(),
        status = 500
    )]
    UnexpectedPages {
        /// The application directory whose `pages/` dir is unexpected.
        app: PathBuf,
    },

    /// A build tool (`cargo` or `wasm-bindgen`) could not be launched.
    #[error(
        code = "BUILD_008",
        message = "could not run '{tool}': {source} (is it installed and on PATH?)",
        status = 500
    )]
    ToolLaunch {
        /// The tool that could not be launched.
        tool: String,
        /// The underlying spawn error.
        #[source]
        source: std::io::Error,
    },

    /// A build tool ran but exited non-zero.
    #[error(
        code = "BUILD_009",
        message = "'{tool}' failed: {message}",
        status = 500
    )]
    ToolFailed {
        /// The tool that failed.
        tool: String,
        /// The tool's captured stderr (trimmed), explaining the failure.
        message: String,
    },

    /// The app fails a page check or an explicit dependency rule.
    #[error(
        code = "BUILD_036",
        message = "page checks refuse this build, {count} finding(s):\n{listing}",
        status = 500
    )]
    ArchitectureRefused {
        /// How many findings there are.
        count: usize,
        /// Every finding, one per line, each led by its code.
        listing: String,
        /// The findings themselves, for a caller that reads their codes.
        #[error(skip_param)]
        findings: Vec<ArchitectureFinding>,
    },

    /// A build script was run without what cargo gives every build script.
    #[error(
        code = "BUILD_037",
        message = "`{variable}` is not set: `PagesBuild::from_env` runs inside a `build.rs`, where cargo sets it",
        status = 500
    )]
    NotInBuildScript {
        /// The variable cargo did not set.
        variable: String,
    },

    /// The pinned Tailwind could not be had or could not build a stylesheet.
    #[error(
        code = "BUILD_038",
        message = "Tailwind could not build the stylesheet: {path}: {reason}",
        status = 500
    )]
    Tailwind {
        /// The file, binary or URL concerned.
        path: String,
        /// What went wrong.
        reason: String,
    },

    /// No `wasm-bindgen` CLI at the version the lockfile binds is installed.
    #[error(
        code = "BUILD_039",
        message = "no wasm-bindgen {locked} is installed ({installed}); scripts/wasm-test.sh installs it under ~/.cache/fundamental/wasm-bindgen-cli",
        status = 500
    )]
    BindgenUnlocked {
        /// The version the lockfile binds.
        locked: String,
        /// What is installed instead.
        installed: String,
    },

    /// A page-tree file declares an `#[island]` while the app's islands live in their own crate.
    #[error(
        code = "BUILD_040",
        message = "{listing}: an `#[island]` in pages/ never hydrates in an app with an islands crate; move it to a domain crate the islands crate uses",
        status = 500
    )]
    IslandInPages {
        /// The files, relative to `pages/`, comma-separated.
        listing: String,
        /// The files themselves.
        #[error(skip_param)]
        files: Vec<PathBuf>,
    },
}

impl BuildError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn island_in_pages(files: Vec<PathBuf>) -> Self {
        let listing = files
            .iter()
            .map(|file| file.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        Self::IslandInPages { listing, files }
    }

    pub(crate) fn architecture_refused(findings: Vec<ArchitectureFinding>) -> Self {
        let listing = findings
            .iter()
            .map(|finding| format!("  {} {finding}", finding.error_code()))
            .collect::<Vec<_>>()
            .join("\n");
        Self::ArchitectureRefused {
            count: findings.len(),
            listing,
            findings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_carries_code_status_and_path() {
        let err = BuildError::io(
            "pages/index.rs",
            std::io::Error::new(std::io::ErrorKind::NotFound, "nope"),
        );
        assert_eq!(err.error_code(), "BUILD_001");
        assert_eq!(err.http_status_code(), 500);
        assert!(err.to_string().contains("pages/index.rs"));
    }

    #[test]
    fn parse_names_the_file_and_message() {
        let err = BuildError::Parse {
            file: PathBuf::from("about.rs"),
            message: "expected `;`".into(),
        };
        assert_eq!(err.error_code(), "BUILD_002");
        assert!(err.to_string().contains("about.rs"));
        assert!(err.to_string().contains("expected `;`"));
    }

    #[test]
    fn reserved_file_explains_the_reason() {
        let err = BuildError::ReservedFile {
            file: PathBuf::from("error.rs"),
            reason: "held for a later port".into(),
        };
        assert_eq!(err.error_code(), "BUILD_003");
        assert!(err.to_string().contains("error.rs"));
    }

    #[test]
    fn duplicate_special_reports_the_kind() {
        let err = BuildError::DuplicateSpecial {
            kind: "document".into(),
            file: PathBuf::from("sub/document.rs"),
        };
        assert_eq!(err.error_code(), "BUILD_004");
        assert!(err.to_string().contains("document"));
    }

    #[test]
    fn missing_pages_is_build_006() {
        let err = BuildError::MissingPages {
            app: PathBuf::from("/app"),
        };
        assert_eq!(err.error_code(), "BUILD_006");
        assert_eq!(err.http_status_code(), 500);
    }

    #[test]
    fn unexpected_pages_is_build_007() {
        let err = BuildError::UnexpectedPages {
            app: PathBuf::from("/app"),
        };
        assert_eq!(err.error_code(), "BUILD_007");
    }

    #[test]
    fn other_catch_all_delegates_to_the_wrapped_error() {
        let inner = BuildError::MissingPages {
            app: PathBuf::from("/app"),
        };
        let err: BuildError = AnyError::new(inner).into();
        assert_eq!(err.error_code(), "BUILD_006");
    }
}
