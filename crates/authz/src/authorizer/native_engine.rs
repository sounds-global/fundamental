//! [`NativeEngine`]: the in-process [`Authorizer`] over the native check engine.

use std::sync::Arc;

use super::{Authorizer, CheckResponse, ExpandResponse, ListObjectsResponse};
use crate::engine::{Engine, EvalLimits, refuse_request_context};
use crate::error::AuthzError;
use crate::schema::{NamespaceName, RelationName, SchemaIr};
use crate::tuple::{CaveatContext, ObjectRef, ReadScope, RelationTuple, SubjectRef, TupleStore};
use crate::zookie::{Consistency, HybridTs, Zookie};

/// The native, in-process authorizer: evaluates the check engine against a local
/// tuple store, and stamps each answer with a [`Zookie`].
#[derive(Debug, Clone)]
pub struct NativeEngine<S> {
    engine: Engine<S>,
}

impl<S: TupleStore> NativeEngine<S> {
    /// Builds a native authorizer over `store` and `schema` with default limits.
    #[must_use]
    pub fn new(store: S, schema: Arc<SchemaIr>) -> Self {
        Self {
            engine: Engine::new(store, schema),
        }
    }

    /// Builds a native authorizer with explicit evaluation limits.
    #[must_use]
    pub fn with_limits(store: S, schema: Arc<SchemaIr>, limits: EvalLimits) -> Self {
        Self {
            engine: Engine::with_limits(store, schema, limits),
        }
    }

    /// Wraps an existing engine.
    #[must_use]
    pub fn from_engine(engine: Engine<S>) -> Self {
        Self { engine }
    }

    /// The engine this authorizer evaluates with.
    #[must_use]
    #[cfg_attr(
        not(feature = "sql"),
        expect(dead_code, reason = "only the `sql` engine reads it")
    )]
    pub(crate) fn engine(&self) -> &Engine<S> {
        &self.engine
    }

    fn zookie(&self, ts: HybridTs) -> Zookie {
        Zookie::new(
            self.engine.store().backend(),
            ts,
            self.engine.schema().hash(),
        )
    }
}

impl<S: TupleStore> Authorizer for NativeEngine<S> {
    async fn check(
        &self,
        object: ObjectRef,
        relation: RelationName,
        subject: SubjectRef,
        context: CaveatContext,
        consistency: Consistency,
    ) -> Result<CheckResponse, AuthzError> {
        // Refused before the index can answer, so it is refused on every path.
        refuse_request_context(&context)?;
        // The set index answers at its watermark, which stamps the zookie.
        if let Some((allowed, watermark)) =
            self.engine
                .try_index(&object, &relation, &subject, &consistency)
        {
            return Ok(CheckResponse {
                allowed,
                evaluated_at: self.zookie(watermark),
            });
        }
        // Read inside the scope, so a retried evaluation reports the read point it observed.
        let (allowed, ts) = self
            .engine
            .store()
            .evaluate(consistency, |scope| {
                let (object, relation, subject, context) = (
                    object.clone(),
                    relation.clone(),
                    subject.clone(),
                    context.clone(),
                );
                Box::pin(async move {
                    let allowed = self
                        .engine
                        .check_in(object, relation, subject, context, &scope)
                        .await?;
                    Ok((allowed, scope.ts()))
                })
            })
            .await?;
        Ok(CheckResponse {
            allowed,
            evaluated_at: self.zookie(ts),
        })
    }

    async fn expand(
        &self,
        object: ObjectRef,
        relation: RelationName,
        consistency: Consistency,
    ) -> Result<ExpandResponse, AuthzError> {
        let (tree, ts) = self
            .engine
            .store()
            .evaluate(consistency, |scope| {
                let (object, relation) = (object.clone(), relation.clone());
                Box::pin(async move {
                    let tree = self.engine.expand_in(object, relation, &scope).await?;
                    Ok((tree, scope.ts()))
                })
            })
            .await?;
        Ok(ExpandResponse {
            tree,
            evaluated_at: self.zookie(ts),
        })
    }

    async fn list_objects(
        &self,
        subject: SubjectRef,
        object_namespace: NamespaceName,
        relation: RelationName,
        context: CaveatContext,
        consistency: Consistency,
        limit: usize,
    ) -> Result<ListObjectsResponse, AuthzError> {
        let (objects, ts) = self
            .engine
            .store()
            .evaluate(consistency, |scope| {
                let (subject, object_namespace, relation, context) = (
                    subject.clone(),
                    object_namespace.clone(),
                    relation.clone(),
                    context.clone(),
                );
                Box::pin(async move {
                    let objects = self
                        .engine
                        .list_objects_in(
                            subject,
                            object_namespace,
                            relation,
                            context,
                            &scope,
                            limit,
                        )
                        .await?;
                    Ok((objects, scope.ts()))
                })
            })
            .await?;
        let evaluated_at = self.zookie(ts);
        Ok(ListObjectsResponse {
            objects,
            evaluated_at,
        })
    }

    async fn write(&self, tuples: &[RelationTuple]) -> Result<Zookie, AuthzError> {
        let ts = self.engine.store().write(tuples).await?;
        Ok(self.zookie(ts))
    }

    async fn delete(&self, tuples: &[RelationTuple]) -> Result<Zookie, AuthzError> {
        let ts = self.engine.store().delete(tuples).await?;
        Ok(self.zookie(ts))
    }
}
