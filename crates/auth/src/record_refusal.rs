//! [`record_refusal`] — the one event a refused token writes.

use fundamental_core::Error as _;
use tracing::info;

use crate::AuthError;

/// Records why a token was refused, once, at `info`: refusals are ordinary anonymous traffic.
pub(crate) fn record_refusal(error: &AuthError) {
    info!(code = error.error_code(), error = %error, "token refused");
}
