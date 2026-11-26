use fundamental_core::outbox::RefusalKind;

use super::ClaimedKey;

/// One row an outbox claim read under its lock.
pub(crate) enum ClaimedRow<M> {
    /// The row decoded into its model.
    Decoded(M),
    /// The row refused to decode; its key was copied before it tried.
    Refused {
        /// The row's whole key, to address it without the model.
        key: ClaimedKey,
        /// The first column that refused.
        column: &'static str,
        /// Why it refused.
        kind: RefusalKind,
    },
}
