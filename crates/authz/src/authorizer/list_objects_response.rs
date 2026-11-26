//! [`ListObjectsResponse`]: the answer to a list-objects.

use serde::{Deserialize, Serialize};

use crate::tuple::ObjectRef;
use crate::zookie::Zookie;

/// The result of a [`list_objects`](crate::authorizer::Authorizer::list_objects):
/// the objects the subject holds the relation on, and the [`Zookie`] they were
/// evaluated at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListObjectsResponse {
    /// The matching objects (capped at the requested limit).
    pub objects: Vec<ObjectRef>,
    /// The consistency token the listing was evaluated at.
    pub evaluated_at: Zookie,
}
