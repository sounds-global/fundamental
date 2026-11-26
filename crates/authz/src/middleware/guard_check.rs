//! [`GuardCheck`]: one permission check, as every route guard makes it.

use std::borrow::Cow;

use fundamental_core::AnyError;
use tracing::{error, warn};

use super::forbidden::Forbidden;
use super::guard_decision::GuardDecision;
use crate::authorizer::DynAuthorizer;
use crate::schema::{Namespace, NamespaceName, Relation, RelationName};
use crate::tuple::{CanonicalId, CaveatContext, ObjectRef, SubjectRef};
use crate::zookie::Consistency;

const DEFAULT_SUBJECT_NAMESPACE: &str = "user";

/// "Does this subject hold this relation on this object?", asked the one way a route guard
/// asks it: canonical object id, principal as a leaf (or the public wildcard), empty caveat
/// context, and [`Consistency::FullConsistency`] by default.
#[derive(Clone)]
pub struct GuardCheck {
    object_namespace: String,
    relation: String,
    subject_namespace: String,
    consistency: Consistency,
    /// The object namespace's canonical form, captured before `N` is erased.
    canonical_id: fn(&str) -> Option<Cow<'_, str>>,
}

impl GuardCheck {
    /// A check requiring `relation` on an object of its namespace `N`.
    #[must_use]
    pub fn of<N>(relation: N) -> Self
    where
        N: Namespace + Relation,
    {
        Self {
            object_namespace: N::NAME.to_owned(),
            relation: relation.relation_name().to_owned(),
            subject_namespace: DEFAULT_SUBJECT_NAMESPACE.to_owned(),
            consistency: Consistency::FullConsistency,
            canonical_id: N::canonical_id,
        }
    }

    /// The namespace of the object this check is about.
    #[must_use]
    pub fn object_namespace(&self) -> &str {
        &self.object_namespace
    }

    /// The relation this check requires.
    #[must_use]
    pub fn relation(&self) -> &str {
        &self.relation
    }

    /// The namespace the principal is treated as a subject of.
    #[must_use]
    pub fn subject_namespace(&self) -> &str {
        &self.subject_namespace
    }

    /// The same check with the principal treated as a subject of `namespace`.
    #[must_use]
    pub fn with_subject_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.subject_namespace = namespace.into();
        self
    }

    /// The same check read at `consistency`.
    #[must_use]
    pub fn with_consistency(mut self, consistency: Consistency) -> Self {
        self.consistency = consistency;
        self
    }

    /// Decides whether `principal` (or, with none, the public wildcard) holds the relation
    /// on the object `raw_id` names; every denial is logged as a security event.
    ///
    /// # Errors
    /// [`Forbidden::MalformedObject`] (`403`) when `raw_id` is not a well-formed id of the
    /// namespace; the authorizer's own failure as itself, never as a denial.
    pub async fn decide(
        &self,
        authorizer: &dyn DynAuthorizer,
        principal: Option<&str>,
        raw_id: &str,
    ) -> Result<GuardDecision, AnyError> {
        // One id, one spelling: reduced through the namespace's form, as `NamespaceExt::object` writes.
        let object_id = (self.canonical_id)(raw_id).ok_or_else(|| {
            warn!(
                object_namespace = %self.object_namespace,
                relation = %self.relation,
                "authorization denied: the object id is not a well-formed id of its namespace"
            );
            AnyError::new(Forbidden::MalformedObject)
        })?;

        let object = ObjectRef::new(
            NamespaceName::new(&self.object_namespace).map_err(AnyError::new)?,
            CanonicalId::new(object_id.as_ref()).map_err(AnyError::new)?,
        );
        let relation = RelationName::new(&self.relation).map_err(AnyError::new)?;
        let subject_namespace =
            NamespaceName::new(&self.subject_namespace).map_err(AnyError::new)?;
        // No principal: ask as the public wildcard, the only way a public route passes.
        let subject = match principal {
            Some(id) => SubjectRef::leaf(
                subject_namespace,
                CanonicalId::new(id).map_err(AnyError::new)?,
            ),
            None => SubjectRef::wildcard(subject_namespace),
        };

        let response = authorizer
            .check(
                object,
                relation,
                subject,
                CaveatContext::new(),
                self.consistency.clone(),
            )
            .await
            .inspect_err(|error| {
                error!(
                    error = %error,
                    object_namespace = %self.object_namespace,
                    object_id = %object_id,
                    relation = %self.relation,
                    "authorization check failed; refusing rather than denying"
                );
            })
            .map_err(AnyError::new)?;

        if response.allowed {
            return Ok(GuardDecision::Allowed {
                object_id: object_id.into_owned(),
            });
        }
        warn!(
            object_namespace = %self.object_namespace,
            object_id = %object_id,
            relation = %self.relation,
            subject_namespace = %self.subject_namespace,
            subject = principal.unwrap_or("<anonymous>"),
            "permission denied"
        );
        Ok(GuardDecision::Denied {
            anonymous: principal.is_none(),
        })
    }
}
