//! [`ExpandResponse`]: the answer to an expand.

use serde::{Deserialize, Serialize};

use crate::engine::UsersetTree;
use crate::zookie::Zookie;

/// The result of an [`expand`](crate::authorizer::Authorizer::expand): the
/// effective subject set as a tree, and the [`Zookie`] it was evaluated at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpandResponse {
    /// The userset tree of who holds the relation.
    pub tree: UsersetTree,
    /// The consistency token the expansion was evaluated at.
    pub evaluated_at: Zookie,
}
