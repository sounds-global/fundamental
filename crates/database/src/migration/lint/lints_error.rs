//! [`LintsError`]: why a project's lint levels could not be read.

use fundamental_core::AnyError;
use fundamental_macros::Error;

/// A refusal from [`Lints::read`](super::Lints::read) or [`Lints::from_toml`](super::Lints::from_toml),
/// naming the file, and the line where the settings loader knows it.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LintsError {
    /// An error from a lower layer that does not map onto a more specific variant.
    Other(AnyError),

    /// The project's settings could not be read.
    #[error(
        code = "LINT_001",
        message = "the migration lints could not be read: {detail}",
        status = 500
    )]
    Unreadable {
        /// What the settings loader said.
        detail: String,
    },

    /// `[migrations.lints]` names a lint there is none of.
    #[error(
        code = "LINT_002",
        message = "{at}: [migrations.lints] names {name}, which is no lint; the lints are \
                   destructive, table_rewrite, key_split_changed, blocking_index, \
                   ddl_beside_data, hot_shard, unindexed_reference, tenant_not_leading, \
                   not_null_without_default, narrowed_type, backfill and large_colocated",
        status = 500
    )]
    UnknownLint {
        /// The file and line.
        at: String,
        /// The name as written.
        name: String,
    },

    /// A lint is set to a level there is none of.
    #[error(
        code = "LINT_003",
        message = "{at}: [migrations.lints] {lint} = {level} is not \"refuse\", \"warn\" or \
                   \"allow\"",
        status = 500
    )]
    UnknownLevel {
        /// The file and line.
        at: String,
        /// The lint.
        lint: String,
        /// The level as written.
        level: String,
    },

    /// A `[migrations]` setting is not of its kind.
    #[error(
        code = "LINT_004",
        message = "{at}: [migrations] {key} is not {expected}",
        status = 500
    )]
    Shape {
        /// The file and line.
        at: String,
        /// The setting.
        key: String,
        /// What it must be.
        expected: String,
    },

    /// A `[migrations]` setting is written in a file beside `fun.toml`; a migrator compiles in
    /// `fun.toml` alone.
    #[error(
        code = "LINT_005",
        message = "{at}: [migrations] is set in fun.toml alone, since a migration refused on a \
                   laptop is refused in the release; move it there",
        status = 500
    )]
    NotInFunToml {
        /// The file and line.
        at: String,
    },
}
