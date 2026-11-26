//! [`DdlError`]: why a statement could not be recognised with confidence.

use fundamental_core::AnyError;
use fundamental_macros::Error;

/// A statement the recogniser could not read confidently enough to judge.
///
/// A key clause that does not parse is a refusal, never a pass, so "could not tell" is never
/// read as "it did".
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DdlError {
    /// An error from a lower layer that does not map onto a more specific variant.
    Other(AnyError),

    /// A `CREATE TABLE` or `CREATE INDEX` whose structure could not be read.
    #[error(
        code = "DDL_001",
        message = "could not read the {kind} statement well enough to judge its key: \
                   {reason}",
        status = 400
    )]
    Unreadable {
        /// What the statement claimed to be — `CREATE TABLE`, `CREATE INDEX`.
        kind: String,
        /// What defeated the recogniser.
        reason: String,
    },

    /// A key list held an element the engine's grammar does not allow.
    #[error(
        code = "DDL_002",
        message = "{object}: the key element {element:?} is not one the engine accepts \
                   — {reason}",
        status = 400
    )]
    MalformedKey {
        /// The table or index the key belongs to.
        object: String,
        /// The element, as written.
        element: String,
        /// Why the grammar rejects it.
        reason: String,
    },
}
