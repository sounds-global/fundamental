//! [`ArchitectureFinding`], one break of the dependency rule.

use fundamental_core::AnyError;
use fundamental_macros::Error;

/// One break of the dependency rule, found while an app was built, located as `path:line`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ArchitectureFinding {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// A view decides at compile time what it is.
    #[error(
        code = "BUILD_027",
        message = "{at}: `{what}` in a view. A view compiles the same for the server and the browser; server-only work belongs in the page's `loader.rs`",
        status = 500
    )]
    CfgInView {
        /// Where, as `path:line`.
        at: String,
        /// What was written: `#[cfg]`, `#[cfg_attr]` or `cfg!`.
        what: String,
    },

    /// A view waits on something.
    #[error(
        code = "BUILD_028",
        message = "{at}: `{what}` in a view. A view lays out the model it is given; loading belongs in the page's `loader.rs`",
        status = 500
    )]
    AsyncInView {
        /// Where, as `path:line`.
        at: String,
        /// What was written: `async` or `.await`.
        what: String,
    },

    /// A view names a dependency explicitly forbidden for its app crate.
    #[error(
        code = "BUILD_029",
        message = "{at}: view names forbidden dependency `{krate}`. Reach it from the page's `loader.rs` and hand the view its result",
        status = 500
    )]
    AdapterInView {
        /// Where, as `path:line`.
        at: String,
        /// The crate, as the view names it.
        krate: String,
        /// The reason it is forbidden.
        ring: String,
    },

    /// A crate depends on a dependency its project explicitly forbids.
    #[error(
        code = "BUILD_030",
        message = "{at}: dependency rule forbids `{from}` → `{to}`",
        status = 500
    )]
    OutwardEdge {
        /// The dependency's line in the depending crate's `Cargo.toml`, as `path:line`.
        at: String,
        /// The depending crate.
        from: String,
        /// Kept for compatibility with existing findings.
        from_ring: String,
        /// The crate it depends on.
        to: String,
        /// Kept for compatibility with existing findings.
        to_ring: String,
    },

    /// A page sits beside a directory of the same name.
    #[error(
        code = "BUILD_032",
        message = "{at}: this page sits beside the directory '{directory}', so a `policy.rs` or `loader.rs` there would not reach it. Move it to that directory's `index.rs`",
        status = 500
    )]
    RouteBesideItsDirectory {
        /// The page, as `path:line`.
        at: String,
        /// The directory of the same name, relative to the project.
        directory: String,
    },

    /// A view pulls in source the check cannot read.
    #[error(
        code = "BUILD_033",
        message = "{at}: `{what}` in a view brings in source this check does not read. Keep the view in its own file, and put what it shares in the app's `src/`",
        status = 500
    )]
    HiddenSource {
        /// Where, as `path:line`.
        at: String,
        /// What was written: `mod x;`, `#[path]` or `include!`.
        what: String,
    },

    /// A pending entry does not hold exactly the findings it froze.
    #[error(
        code = "BUILD_034",
        message = "{at}: the pending entry for '{file}' freezes {frozen} {rule} finding(s) and {found}. {advice}",
        status = 500
    )]
    PendingMismatch {
        /// The entry in the table, as `path:line`.
        at: String,
        /// The file it freezes, relative to the project.
        file: String,
        /// The rule's code.
        rule: String,
        /// The count it freezes.
        frozen: u32,
        /// What the check found instead.
        found: String,
        /// What to do.
        advice: String,
    },

    /// The dependency rule configuration cannot be read as written.
    #[error(code = "BUILD_035", message = "{at}: {reason}", status = 500)]
    InvalidTable {
        /// Where, as `path:line`, or the table's file.
        at: String,
        /// What is wrong.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    #[test]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn each_finding_carries_its_code_and_names_where_it_is() {
        let cases = [
            (
                ArchitectureFinding::CfgInView {
                    at: "apps/a/pages/x.rs:3".into(),
                    what: "cfg!".into(),
                },
                "BUILD_027",
            ),
            (
                ArchitectureFinding::AsyncInView {
                    at: "apps/a/pages/x.rs:3".into(),
                    what: ".await".into(),
                },
                "BUILD_028",
            ),
            (
                ArchitectureFinding::AdapterInView {
                    at: "apps/a/pages/x.rs:3".into(),
                    krate: "store".into(),
                    ring: "adapters".into(),
                },
                "BUILD_029",
            ),
            (
                ArchitectureFinding::OutwardEdge {
                    at: "domain/Cargo.toml:9".into(),
                    from: "domain".into(),
                    from_ring: "entities".into(),
                    to: "store".into(),
                    to_ring: "adapters".into(),
                },
                "BUILD_030",
            ),
            (
                ArchitectureFinding::RouteBesideItsDirectory {
                    at: "apps/a/pages/x.rs:1".into(),
                    directory: "apps/a/pages/x".into(),
                },
                "BUILD_032",
            ),
            (
                ArchitectureFinding::HiddenSource {
                    at: "apps/a/pages/x.rs:2".into(),
                    what: "include!".into(),
                },
                "BUILD_033",
            ),
            (
                ArchitectureFinding::PendingMismatch {
                    at: "fundamental/architecture.toml:4".into(),
                    file: "apps/a/pages/x.rs".into(),
                    rule: "BUILD_027".into(),
                    frozen: 2,
                    found: "found 3".into(),
                    advice: "Fix the new one.".into(),
                },
                "BUILD_034",
            ),
            (
                ArchitectureFinding::InvalidTable {
                    at: "fundamental/architecture.toml:2".into(),
                    reason: "`x` is placed twice".into(),
                },
                "BUILD_035",
            ),
        ];
        for (finding, code) in cases {
            assert_eq!(finding.error_code(), code);
            assert_eq!(finding.http_status_code(), 500);
            assert!(!finding.is_retryable());
            assert!(finding.to_string().contains(".toml:") || finding.to_string().contains(".rs:"));
        }
    }
}
