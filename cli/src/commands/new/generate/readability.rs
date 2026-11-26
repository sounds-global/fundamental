//! The readability lints a new project starts with, and their bars.

/// The lints that bound what a reader holds in mind to follow an item, each at `warn` in the
/// project's workspace lints; their bars are [`READABILITY_THRESHOLDS`] in `clippy.toml`.
pub(super) const READABILITY_LINTS: [&str; 11] = [
    "clippy::too_many_lines",
    "clippy::cognitive_complexity",
    "clippy::too_many_arguments",
    "clippy::excessive_nesting",
    "clippy::fn_params_excessive_bools",
    "clippy::struct_excessive_bools",
    "clippy::missing_errors_doc",
    "clippy::missing_panics_doc",
    "missing_docs",
    "clippy::allow_attributes",
    "clippy::allow_attributes_without_reason",
];

/// Each bar's `clippy.toml` key and value: a function fits on one screen and reads as one idea.
pub(super) const READABILITY_THRESHOLDS: [(&str, u32); 4] = [
    ("too-many-lines-threshold", 60),
    ("cognitive-complexity-threshold", 15),
    ("too-many-arguments-threshold", 5),
    ("excessive-nesting-threshold", 4),
];
