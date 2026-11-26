//! [`GuardError`]: why a migration set was refused before any of it ran.

use fundamental_core::AnyError;
use fundamental_macros::Error;

use crate::migration::ddl::DdlError;
use crate::migration::lint::{Lint, LintFinding};

/// A refusal from [`MigrationGuard`](crate::migration::MigrationGuard).
///
/// Every variant names the migration, the object and the exact text at fault. `GUARD_005`
/// and `GUARD_006` are retired codes, now lints refused as [`Linted`](GuardError::Linted).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GuardError {
    /// An error from a lower layer that does not map onto a more specific variant.
    Other(AnyError),

    /// A key element named a column and left its distribution to the engine.
    #[error(
        code = "GUARD_001",
        message = "{migration}: {object} declares the key element {element:?} without \
                   HASH, ASC or DESC. The engine will choose — measured, it hashes the \
                   leading column — and the migration will not say which. Write the \
                   sharding you mean: HASH for a point lookup, ASC or DESC where the \
                   access pattern is an ordered or ranged scan.",
        status = 400
    )]
    UndeclaredKey {
        /// The migration file.
        migration: String,
        /// The table or index whose key it is.
        object: String,
        /// The element, as written.
        element: String,
    },

    /// A column carried an inline `PRIMARY KEY`, which has no room for a sharding token.
    #[error(
        code = "GUARD_002",
        message = "{migration}: {table}.{column} declares PRIMARY KEY inline, and the \
                   engine's grammar gives that form no room to shard — `{column} … \
                   PRIMARY KEY HASH` is a syntax error. Move it to a table-level \
                   `PRIMARY KEY ({column} HASH)` (or ASC/DESC) so the key can say how it \
                   distributes.",
        status = 400
    )]
    InlinePrimaryKey {
        /// The migration file.
        migration: String,
        /// The table.
        table: String,
        /// The column carrying the inline key.
        column: String,
    },

    /// A `UNIQUE` constraint, which becomes an index that cannot declare its sharding.
    #[error(
        code = "GUARD_003",
        message = "{migration}: {table} declares UNIQUE ({columns}). That becomes a \
                   unique index hashed on its leading column, and a UNIQUE constraint \
                   cannot carry a sharding token in any form — UNIQUE HASH, \
                   UNIQUE (c HASH) and UNIQUE (c ASC) are all syntax errors. Write it as \
                   a separate CREATE UNIQUE INDEX, where the key can be declared.",
        status = 400
    )]
    UniqueConstraint {
        /// The migration file.
        migration: String,
        /// The table.
        table: String,
        /// The constrained columns, rendered.
        columns: String,
    },

    /// A hash component whose own `CHECK` proves it cannot spread across tablets.
    #[error(
        code = "GUARD_004",
        message = "{migration}: {object} hashes on ({columns}), which a CHECK constraint \
                   bounds to at most {bound} distinct value(s) — below the floor of \
                   {floor}. Hashing a near-constant column puts every row on one tablet. \
                   Widen the hash component to include a high-cardinality column, or \
                   range-shard it with ASC/DESC.",
        status = 400
    )]
    HotHashComponent {
        /// The migration file.
        migration: String,
        /// The table or index.
        object: String,
        /// The hash component's columns, rendered.
        columns: String,
        /// What the CHECK constraints bound the component to.
        bound: u64,
        /// The floor this fleet requires.
        floor: u64,
    },

    /// The statement could not be read confidently enough to judge.
    ///
    /// Never a pass.
    #[error(code = "GUARD_007", message = "{migration}: {source}", status = 400)]
    Unreadable {
        /// The migration file.
        migration: String,
        /// What defeated the recogniser.
        #[source]
        source: DdlError,
    },

    /// Lints that block under the project's levels, every one of them.
    #[error(
        code = "GUARD_008",
        message = "the migration lints refuse this set:\n{report}",
        status = 400
    )]
    Linted {
        /// The findings, one a line.
        report: String,
        /// The findings.
        findings: Vec<LintFinding>,
    },
}

impl GuardError {
    /// The lints that refused the set; none for a refusal by a fixed rule.
    #[must_use]
    pub fn lints(&self) -> Vec<Lint> {
        match self {
            Self::Linted { findings, .. } => findings.iter().map(|finding| finding.lint).collect(),
            _ => Vec::new(),
        }
    }
}
