//! [`FileRule`], a rule judged one source file at a time.

use std::fmt;

use fundamental_core::Variant;
use fundamental_macros::Variant;
use fundamental_manifest::pending::PendingRule;

/// A rule judged one file at a time, which a pending entry may freeze; its value is its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Variant)]
pub(crate) enum FileRule {
    /// `BUILD_027`: `#[cfg]` or `cfg!` in a view.
    #[variant(value = "BUILD_027")]
    Cfg,
    /// `BUILD_028`: `async` or `.await` in a view.
    #[variant(value = "BUILD_028")]
    Async,
    /// `BUILD_029`: a view names a crate placed among adapters or drivers.
    #[variant(value = "BUILD_029")]
    Adapter,
    /// `BUILD_030`: a crate's `Cargo.toml` depends on a crate in a ring further out.
    #[variant(value = "BUILD_030")]
    OutwardEdge,
    /// `BUILD_032`: `x.rs` beside a directory `x/`.
    #[variant(value = "BUILD_032")]
    RouteBesideItsDirectory,
    /// `BUILD_033`: a view pulls in source the check cannot read.
    #[variant(value = "BUILD_033")]
    HiddenSource,
}

impl PendingRule for FileRule {
    /// A frozen finding toward a crate names it, so debt toward one crate cannot be swapped.
    fn name_key(self) -> Option<&'static str> {
        matches!(self, Self::Adapter | Self::OutwardEdge).then_some("crate")
    }
}

impl fmt::Display for FileRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_rule_reads_back_from_its_code() {
        for rule in FileRule::OPTIONS {
            assert_eq!(FileRule::from_value(rule.value()), Some(*rule));
        }
    }

    #[test]
    fn a_crate_rule_cannot_be_frozen() {
        for code in ["BUILD_031", "BUILD_034", "BUILD_035", "cfg", ""] {
            assert_eq!(FileRule::from_value(code), None, "{code}");
        }
    }
}
