//! [`CheckResponse`]: the answer to a permission check.

use serde::{Deserialize, Serialize};

use crate::zookie::Zookie;

/// The result of a [`check`](crate::authorizer::Authorizer::check): whether access
/// is allowed, and the [`Zookie`] the decision was evaluated at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckResponse {
    /// Whether the subject may perform the relation on the object.
    pub allowed: bool,
    /// The consistency token the decision was evaluated at.
    pub evaluated_at: Zookie,
}
