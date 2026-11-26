//! [`AuthorizerExt`]: the fluent, typed authorization surface.

use std::future::Future;

use super::{CheckBuilder, DynAuthorizer, ListBuilder};
use crate::error::AuthzError;
use crate::schema::{Namespace, NamespaceExt, NamespaceName, Relation, RelationName};
use crate::tuple::{IntoCanonicalId, IntoSubject, ObjectRef, RelationTuple, SubjectRef};
use crate::zookie::Zookie;

/// The ergonomic authorization API over the typed schema, read as `(subject, relation,
/// object)`: the subject is anything [`IntoSubject`], the object anything
/// [`IntoCanonicalId`]. Uses no hand-built [`ObjectRef`], [`RelationName`] or [`SubjectRef`].
///
/// ```no_run
/// # extern crate fundamental_authz;
/// # extern crate fundamental_macros;
/// # use fundamental_authz::AuthzError;
/// # use fundamental_authz::authorizer::{AuthorizerExt, DynAuthorizer};
/// # use fundamental_authz::schema::{NamespaceExt, RelationExt};
/// # use fundamental_macros::Namespace;
/// # #[derive(Namespace)] enum User {}
/// # #[derive(Namespace)] enum Document {
/// #     #[relation(direct(User))] Owner,
/// #     #[relation(union(Owner, direct(User)))] Viewer,
/// # }
/// # async fn demo(authz: &dyn DynAuthorizer) -> Result<(), AuthzError> {
/// authz.grant(User::subject("alice"), Document::Owner, "readme").await?;
/// if authz.can(User::subject("alice"), Document::Viewer, "readme").await? { /* … */ }
/// let zookie = authz.grant(User::subject("bob"), Document::Owner, "spec").await?;
/// authz.can(User::subject("bob"), Document::Viewer, "spec").at_least_as_fresh(zookie).await?;
/// let docs = authz.list(User::subject("alice"), Document::Viewer).await?;
/// # Ok(())
/// # }
/// ```
///
/// Implemented for every [`Authorizer`](super::Authorizer) and for `Arc<dyn DynAuthorizer>`.
pub trait AuthorizerExt {
    /// Whether `subject` may perform `relation` on `object`.
    ///
    /// Returns a builder that awaits to the bare `bool`; `.response()` yields the full
    /// [`CheckResponse`](super::CheckResponse). Not named `check`, so it never collides with
    /// [`Authorizer::check`](super::Authorizer::check). Errors surface when awaited.
    fn can<N>(
        &self,
        subject: impl IntoSubject,
        relation: N,
        object: impl IntoCanonicalId,
    ) -> CheckBuilder<'_, Self>
    where
        N: Namespace + Relation;

    /// Grants `relation` on `object` to `subject`, returning the commit zookie.
    ///
    /// # Errors
    ///
    /// Surfaces an [`AuthzError`] for an invalid reference or a write failure.
    fn grant<N>(
        &self,
        subject: impl IntoSubject,
        relation: N,
        object: impl IntoCanonicalId,
    ) -> impl Future<Output = Result<Zookie, AuthzError>> + Send
    where
        N: Namespace + Relation;

    /// Revokes `relation` on `object` from `subject`, returning the commit zookie.
    ///
    /// # Errors
    ///
    /// Surfaces an [`AuthzError`] for an invalid reference or a delete failure.
    fn revoke<N>(
        &self,
        subject: impl IntoSubject,
        relation: N,
        object: impl IntoCanonicalId,
    ) -> impl Future<Output = Result<Zookie, AuthzError>> + Send
    where
        N: Namespace + Relation;

    /// Lists the objects of `relation`'s namespace on which `subject` holds it,
    /// returning a builder (default limit `1000`).
    fn list<N>(&self, subject: impl IntoSubject, relation: N) -> ListBuilder<'_, Self>
    where
        N: Namespace + Relation;
}

impl<T: DynAuthorizer + ?Sized> AuthorizerExt for T {
    fn can<N>(
        &self,
        subject: impl IntoSubject,
        relation: N,
        object: impl IntoCanonicalId,
    ) -> CheckBuilder<'_, Self>
    where
        N: Namespace + Relation,
    {
        CheckBuilder::new(self, build_refs::<N>(subject, relation, object))
    }

    fn grant<N>(
        &self,
        subject: impl IntoSubject,
        relation: N,
        object: impl IntoCanonicalId,
    ) -> impl Future<Output = Result<Zookie, AuthzError>> + Send
    where
        N: Namespace + Relation,
    {
        let prepared = build_refs::<N>(subject, relation, object);
        async move {
            let (object, relation, subject) = prepared?;
            self.write(&[RelationTuple::new(object, relation, subject)])
                .await
        }
    }

    fn revoke<N>(
        &self,
        subject: impl IntoSubject,
        relation: N,
        object: impl IntoCanonicalId,
    ) -> impl Future<Output = Result<Zookie, AuthzError>> + Send
    where
        N: Namespace + Relation,
    {
        let prepared = build_refs::<N>(subject, relation, object);
        async move {
            let (object, relation, subject) = prepared?;
            self.delete(&[RelationTuple::new(object, relation, subject)])
                .await
        }
    }

    fn list<N>(&self, subject: impl IntoSubject, relation: N) -> ListBuilder<'_, Self>
    where
        N: Namespace + Relation,
    {
        ListBuilder::new(self, build_list_refs::<N>(subject, relation))
    }
}

/// Resolves the `(object, relation, subject)` a check, grant or revoke needs.
pub(super) fn build_refs<N: Namespace + Relation>(
    subject: impl IntoSubject,
    relation: N,
    object: impl IntoCanonicalId,
) -> Result<(ObjectRef, RelationName, SubjectRef), AuthzError> {
    let object = N::object(object)?;
    let relation = RelationName::new(relation.relation_name())
        .expect("derive guarantees a valid relation identifier");
    let subject = subject.into_subject()?;
    Ok((object, relation, subject))
}

fn build_list_refs<N: Namespace + Relation>(
    subject: impl IntoSubject,
    relation: N,
) -> Result<(SubjectRef, NamespaceName, RelationName), AuthzError> {
    let subject = subject.into_subject()?;
    let namespace = N::name();
    let relation = RelationName::new(relation.relation_name())
        .expect("derive guarantees a valid relation identifier");
    Ok((subject, namespace, relation))
}
