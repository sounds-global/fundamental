//! [`Engine`]: the recursive check evaluator over a [`TupleStore`] and a schema.

use std::collections::{BTreeSet, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};
use tracing::{error, instrument, warn};

use super::caveat_eval;
use super::check_path::CheckPath;
use super::eval_context::{EvalContext, plausible_now};
use super::eval_limits::EvalLimits;
use super::refuse_request_context;
use super::set_index::SetIndex;
use super::userset_tree::UsersetTree;
use super::{Justification, Witness};
use crate::error::AuthzError;
use crate::schema::{NamespaceName, RelationName, RewriteIr, SchemaIr};
use crate::tuple::{
    Caveat, CaveatContext, DirectEdge, ObjectRef, ReadScope, RelationTuple, SubjectRef, TupleStore,
    WallTime,
};
use crate::zookie::{Consistency, HybridTs};

/// A boxed evaluation future, so the recursive walk can call itself across `await` points.
/// `None` is a denial; `Some(j)` an allow and what justified it.
type OutcomeFuture<'a, J> =
    Pin<Box<dyn Future<Output = Result<Option<J>, AuthzError>> + Send + 'a>>;

/// A boxed expansion future, for the same reason as [`OutcomeFuture`].
type TreeFuture<'a> = Pin<Box<dyn Future<Output = Result<UsersetTree, AuthzError>> + Send + 'a>>;

/// The invariants carried through one check's recursion; two references, so it is `Copy`.
struct CheckWalk<'a, K: ReadScope, J: Justification> {
    subject: &'a SubjectRef,
    ctx: &'a EvalContext<'a, K, J>,
}

// Hand-written: deriving would demand `K: Copy` and `J: Copy`.
impl<K: ReadScope, J: Justification> Clone for CheckWalk<'_, K, J> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: ReadScope, J: Justification> Copy for CheckWalk<'_, K, J> {}

/// The authorization check engine: evaluates the Zanzibar rewrite algebra over a
/// compiled [`SchemaIr`] and a [`TupleStore`].
///
/// Kept finite by the [`EvalLimits`]. A clean miss is a denial; a bound hit or a backend
/// failure is an error, never a silent deny.
#[derive(Debug, Clone)]
pub struct Engine<S> {
    store: S,
    schema: Arc<SchemaIr>,
    limits: EvalLimits,
    set_index: Option<Arc<dyn SetIndex>>,
}

impl<S: TupleStore> Engine<S> {
    /// Builds an engine with the default [`EvalLimits`].
    #[must_use]
    pub fn new(store: S, schema: Arc<SchemaIr>) -> Self {
        Self {
            store,
            schema,
            limits: EvalLimits::default(),
            set_index: None,
        }
    }

    /// Builds an engine with explicit limits.
    #[must_use]
    pub fn with_limits(store: S, schema: Arc<SchemaIr>, limits: EvalLimits) -> Self {
        Self {
            store,
            schema,
            limits,
            set_index: None,
        }
    }

    /// Attaches a precomputed [`SetIndex`] to accelerate membership checks.
    ///
    /// Consulted only where [`try_index`](Engine::try_index) finds it safe; every other check
    /// evaluates authoritatively.
    #[must_use]
    pub fn with_set_index(mut self, set_index: Arc<dyn SetIndex>) -> Self {
        self.set_index = Some(set_index);
        self
    }

    /// Answers a check from the [`SetIndex`], with the watermark it is valid at, or `None` when
    /// the caller must evaluate authoritatively.
    ///
    /// Only for a covered relation, a concrete leaf subject, and a consistency the watermark
    /// satisfies.
    #[must_use]
    pub fn try_index(
        &self,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
        consistency: &Consistency,
    ) -> Option<(bool, HybridTs)> {
        let index = self.set_index.as_ref()?;
        if !index_fresh_enough(consistency, index.watermark()) {
            return None;
        }
        let decision = index.contains(object, relation, subject)?;
        Some((decision, index.watermark()))
    }

    /// The compiled schema this engine evaluates against.
    #[must_use]
    pub fn schema(&self) -> &Arc<SchemaIr> {
        &self.schema
    }

    /// The limits this engine walks within.
    #[must_use]
    #[cfg_attr(
        not(feature = "sql"),
        expect(dead_code, reason = "only the `sql` engine reads it")
    )]
    pub(crate) fn limits(&self) -> EvalLimits {
        self.limits
    }

    /// The underlying tuple store, for a wrapper such as the
    /// [`Authorizer`](crate::authorizer::Authorizer).
    #[must_use]
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Returns whether `subject` has `relation` on `object`, evaluating every caveat
    /// against its tuple's stored arguments and the evaluation's own `now`.
    ///
    /// # Errors
    ///
    /// [`AuthzError::RequestCaveatParameter`] if `context` binds any parameter;
    /// [`AuthzError::ImplausibleClock`] if the read scope's `now` is not plausible;
    /// [`AuthzError::UndefinedNamespace`] / [`AuthzError::UndefinedRelation`] for an unknown
    /// object type or relation; [`AuthzError::DepthExceeded`] /
    /// [`AuthzError::BudgetExceeded`] if a bound is hit; [`AuthzError::CaveatEval`] for a
    /// malformed caveat; or [`AuthzError::Storage`] for a backend failure.
    #[instrument(skip(self, context), fields(object = %object, relation = %relation, subject = %subject))]
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub async fn check(
        &self,
        object: ObjectRef,
        relation: RelationName,
        subject: SubjectRef,
        context: CaveatContext,
        consistency: Consistency,
    ) -> Result<bool, AuthzError> {
        refuse_request_context(&context)?;
        if let Some((decision, _)) = self.try_index(&object, &relation, &subject, &consistency) {
            return Ok(decision);
        }
        self.store
            .evaluate(consistency, |scope| {
                let (object, relation, subject, context) = (
                    object.clone(),
                    relation.clone(),
                    subject.clone(),
                    context.clone(),
                );
                Box::pin(async move {
                    self.check_in(object, relation, subject, context, &scope)
                        .await
                })
            })
            .await
    }

    /// Evaluates a check inside an already-open read scope.
    ///
    /// Lets the [`Authorizer`](crate::authorizer::Authorizer) read the snapshot's timestamp for
    /// the zookie without a second snapshot.
    ///
    /// # Errors
    ///
    /// Same as [`check`](Engine::check).
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub async fn check_in(
        &self,
        object: ObjectRef,
        relation: RelationName,
        subject: SubjectRef,
        context: CaveatContext,
        scope: &S::Scope,
    ) -> Result<bool, AuthzError> {
        // An unknown object type or relation is an error, not a denial that would hide the typo.
        let namespace = self.schema.namespace(&object.namespace).ok_or_else(|| {
            AuthzError::UndefinedNamespace {
                namespace: object.namespace.to_string(),
            }
        })?;
        if namespace.relation(&relation).is_none() {
            return Err(AuthzError::UndefinedRelation {
                namespace: object.namespace.to_string(),
                relation: relation.to_string(),
            });
        }

        refuse_request_context(&context)?;
        let ctx = EvalContext::<_, ()>::new(self.limits, scope)?;
        let walk = CheckWalk {
            subject: &subject,
            ctx: &ctx,
        };
        Ok(self
            .check_inner(&object, &relation, 0, CheckPath::root(), walk)
            .await?
            .is_some())
    }

    /// The tuples an allow of `subject` holding `relation` on `object` rests on, read
    /// inside an already-open read scope, or `None` for a denial.
    ///
    /// The check's own walk, keeping each allow's [`Witness`] and refusing to evaluate an
    /// exclusion. The witness is the walk's last read of each tuple, not a lock on it.
    ///
    /// # Errors
    ///
    /// [`AuthzError::RelationNotLocked`] if the relation is not declared locked;
    /// [`AuthzError::ExclusionInLockedWalk`] if the walk reaches an exclusion; otherwise
    /// the errors of [`check_in`](Engine::check_in).
    #[cfg_attr(
        not(feature = "sql"),
        expect(dead_code, reason = "only the `sql` engine reads it")
    )]
    pub(crate) async fn witness_in(
        &self,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
        scope: &S::Scope,
    ) -> Result<(Option<Witness>, usize), AuthzError> {
        let namespace = self.schema.namespace(&object.namespace).ok_or_else(|| {
            AuthzError::UndefinedNamespace {
                namespace: object.namespace.to_string(),
            }
        })?;
        let declared =
            namespace
                .relation(relation)
                .ok_or_else(|| AuthzError::UndefinedRelation {
                    namespace: object.namespace.to_string(),
                    relation: relation.to_string(),
                })?;
        if !declared.locked {
            warn!(namespace = %object.namespace, relation = %relation, "locked check refused: the relation is not declared #[locked]");
            return Err(AuthzError::RelationNotLocked {
                namespace: object.namespace.to_string(),
                relation: relation.to_string(),
            });
        }
        let ctx = EvalContext::<_, Witness>::new(self.limits, scope)?;
        let walk = CheckWalk { subject, ctx: &ctx };
        let witness = self
            .check_inner(object, relation, 0, CheckPath::root(), walk)
            .await?;
        Ok((witness, ctx.reads_used()))
    }

    /// Memoized, cycle-safe evaluation of one `object#relation@subject` question.
    ///
    /// `path` is taken by value so each concurrent branch has its own ancestors; the memo and
    /// read budget are shared.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn check_inner<'a, J: Justification>(
        &'a self,
        object: &'a ObjectRef,
        relation: &'a RelationName,
        depth: usize,
        path: CheckPath,
        walk: CheckWalk<'a, S::Scope, J>,
    ) -> OutcomeFuture<'a, J> {
        Box::pin(async move {
            if depth > walk.ctx.limits().max_depth {
                return Err(AuthzError::DepthExceeded {
                    depth: walk.ctx.limits().max_depth,
                });
            }
            let key = (object.clone(), relation.clone(), walk.subject.clone());
            if let Some(decision) = walk.ctx.memoized(&key) {
                return Ok(decision);
            }
            // A relation already on this branch's path is a cycle: deny this branch, and do not memoize,
            // since the answer belongs to the path, not the key.
            if path.contains(&key) {
                return Ok(None);
            }
            let child_path = path.push(key.clone());
            let decision = self
                .eval_relation(object, relation, depth, child_path, walk)
                .await?;
            walk.ctx.memoize(key, decision.clone());
            Ok(decision)
        })
    }

    /// Existential fan-out: the first branch to grant wins; a miss from every branch is a denial;
    /// the first error propagates. Concurrent when [`parallel_fanout`](EvalLimits::parallel_fanout)
    /// is set, with the same decision either way.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn any_true<'a, J: Justification>(
        &self,
        parallel: bool,
        branches: Vec<OutcomeFuture<'a, J>>,
    ) -> Result<Option<J>, AuthzError> {
        if parallel {
            let mut pending: FuturesUnordered<_> = branches.into_iter().collect();
            while let Some(result) = pending.next().await {
                if let Some(justification) = result? {
                    return Ok(Some(justification));
                }
            }
            Ok(None)
        } else {
            // An un-polled future has performed no reads, so returning early drops the rest unread.
            for branch in branches {
                if let Some(justification) = branch.await? {
                    return Ok(Some(justification));
                }
            }
            Ok(None)
        }
    }

    /// Looks up the relation's rewrite and evaluates it; a relation missing at runtime (out-of-band
    /// data) contributes nothing.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    async fn eval_relation<J: Justification>(
        &self,
        object: &ObjectRef,
        relation: &RelationName,
        depth: usize,
        path: CheckPath,
        walk: CheckWalk<'_, S::Scope, J>,
    ) -> Result<Option<J>, AuthzError> {
        let Some(namespace) = self.schema.namespace(&object.namespace) else {
            return Ok(None);
        };
        let Some(relation_ir) = namespace.relation(relation) else {
            return Ok(None);
        };
        self.eval_rewrite(object, relation, &relation_ir.rewrite, depth, path, walk)
            .await
    }

    /// Evaluates one rewrite node; `relation` is what a `_this` node reads.
    #[expect(
        clippy::too_many_arguments,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn eval_rewrite<'a, J: Justification>(
        &'a self,
        object: &'a ObjectRef,
        relation: &'a RelationName,
        rewrite: &'a RewriteIr,
        depth: usize,
        path: CheckPath,
        walk: CheckWalk<'a, S::Scope, J>,
    ) -> OutcomeFuture<'a, J> {
        Box::pin(async move {
            match rewrite {
                RewriteIr::This => self.eval_this(object, relation, depth, path, walk).await,
                RewriteIr::ComputedUserset { relation: computed } => {
                    self.check_inner(object, computed, depth + 1, path, walk)
                        .await
                }
                RewriteIr::TupleToUserset { tupleset, computed } => {
                    self.eval_ttu(object, tupleset, computed, depth, path, walk)
                        .await
                }
                RewriteIr::Union(children) => {
                    let branches = children
                        .iter()
                        .map(|child| {
                            self.eval_rewrite(object, relation, child, depth, path.clone(), walk)
                        })
                        .collect();
                    self.any_true(walk.ctx.limits().parallel_fanout, branches)
                        .await
                }
                RewriteIr::Intersection(children) => {
                    // Every child must grant, so short-circuit on the first miss; kept sequential.
                    let mut joined = J::unit();
                    for child in children {
                        let Some(justification) = self
                            .eval_rewrite(object, relation, child, depth, path.clone(), walk)
                            .await?
                        else {
                            return Ok(None);
                        };
                        joined = joined.join(justification);
                    }
                    Ok(Some(joined))
                }
                RewriteIr::Exclusion { .. } if !J::ADMITS_EXCLUSION => {
                    error!(namespace = %object.namespace, relation = %relation, "locked walk refused: it reached an exclusion the schema's locked closure does not");
                    Err(AuthzError::ExclusionInLockedWalk {
                        namespace: object.namespace.to_string(),
                        relation: relation.to_string(),
                    })
                }
                RewriteIr::Exclusion { include, exclude } => {
                    let Some(justification) = self
                        .eval_rewrite(object, relation, include, depth, path.clone(), walk)
                        .await?
                    else {
                        return Ok(None);
                    };
                    let excluded = self
                        .eval_rewrite(object, relation, exclude, depth, path, walk)
                        .await?;
                    Ok(excluded.is_none().then_some(justification))
                }
            }
        })
    }

    /// Evaluates a `_this` node: the direct subject, the public wildcard, then the stored usersets.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    async fn eval_this<J: Justification>(
        &self,
        object: &ObjectRef,
        relation: &RelationName,
        depth: usize,
        path: CheckPath,
        walk: CheckWalk<'_, S::Scope, J>,
    ) -> Result<Option<J>, AuthzError> {
        let ctx = walk.ctx;
        let subject = walk.subject;
        // 1. The exact subject, as a point lookup: the common fast path.
        ctx.charge_read()?;
        if let Some(edge) = self
            .store
            .read_edge(ctx.scope(), object, relation, subject)
            .await?
            && self.edge_grants(&edge, &object.namespace, ctx)?
        {
            return Ok(Some(J::edge(|| stored(object, relation, subject, edge))));
        }
        // 2. The public wildcard, only for a concrete leaf subject.
        if !subject.is_userset() && !subject.is_wildcard() {
            let wildcard = SubjectRef::wildcard(subject.object.namespace.clone());
            ctx.charge_read()?;
            if let Some(edge) = self
                .store
                .read_edge(ctx.scope(), object, relation, &wildcard)
                .await?
                && self.edge_grants(&edge, &object.namespace, ctx)?
            {
                return Ok(Some(J::edge(|| stored(object, relation, &wildcard, edge))));
            }
        }
        // 3. The stored usersets, recursed into concurrently after the synchronous caveat gate.
        ctx.charge_read()?;
        let usersets = self
            .store
            .read_usersets(ctx.scope(), object, relation, ctx.limits().page_limit)
            .await?;
        let mut targets: Vec<(RelationTuple, RelationName)> = Vec::new();
        for tuple in usersets {
            if let Some(caveat) = &tuple.caveat
                && !self.caveat_grants(caveat, &object.namespace, ctx)?
            {
                continue;
            }
            if let Some(userset_relation) = tuple.subject.relation.clone() {
                targets.push((tuple, userset_relation));
            }
        }
        let branches = targets
            .iter()
            .map(|(tuple, userset_relation)| {
                let branch = self.check_inner(
                    &tuple.subject.object,
                    userset_relation,
                    depth + 1,
                    path.clone(),
                    walk,
                );
                through(tuple, branch)
            })
            .collect();
        self.any_true(ctx.limits().parallel_fanout, branches).await
    }

    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    async fn eval_ttu<J: Justification>(
        &self,
        object: &ObjectRef,
        tupleset: &RelationName,
        computed: &RelationName,
        depth: usize,
        path: CheckPath,
        walk: CheckWalk<'_, S::Scope, J>,
    ) -> Result<Option<J>, AuthzError> {
        let ctx = walk.ctx;
        ctx.charge_read()?;
        let edges = self
            .store
            .read_all(ctx.scope(), object, tupleset, ctx.limits().page_limit)
            .await?;
        // Each reached object is an independent hop, checked concurrently after the caveat gate.
        let mut targets: Vec<RelationTuple> = Vec::new();
        for tuple in edges {
            if let Some(caveat) = &tuple.caveat
                && !self.caveat_grants(caveat, &object.namespace, ctx)?
            {
                continue;
            }
            targets.push(tuple);
        }
        let branches = targets
            .iter()
            .map(|tuple| {
                let branch = self.check_inner(
                    &tuple.subject.object,
                    computed,
                    depth + 1,
                    path.clone(),
                    walk,
                );
                through(tuple, branch)
            })
            .collect();
        self.any_true(ctx.limits().parallel_fanout, branches).await
    }

    fn edge_grants<J: Justification>(
        &self,
        edge: &DirectEdge,
        object_namespace: &NamespaceName,
        ctx: &EvalContext<'_, S::Scope, J>,
    ) -> Result<bool, AuthzError> {
        match edge {
            DirectEdge::Unconditional => Ok(true),
            DirectEdge::Conditional(caveat) => self.caveat_grants(caveat, object_namespace, ctx),
        }
    }

    /// Evaluates a caveat against the tuple's stored arguments with the evaluation's `now`.
    fn caveat_grants<J: Justification>(
        &self,
        caveat: &Caveat,
        object_namespace: &NamespaceName,
        ctx: &EvalContext<'_, S::Scope, J>,
    ) -> Result<bool, AuthzError> {
        self.caveat_holds(caveat, object_namespace, ctx.now())
    }

    /// Whether a stored `tuple` grants at `now`: always when it is unconditional, and when
    /// its caveat holds at `now` otherwise.
    ///
    /// A locked check judges each witness tuple again with this after taking its locks.
    ///
    /// # Errors
    ///
    /// [`AuthzError::ImplausibleClock`] if `now` is not plausible;
    /// [`AuthzError::UndefinedNamespace`] / [`AuthzError::UndefinedCaveat`] /
    /// [`AuthzError::CaveatEval`] as a check's caveat evaluation.
    #[cfg_attr(
        not(feature = "sql"),
        expect(dead_code, reason = "only the `sql` engine reads it")
    )]
    pub(crate) fn tuple_grants_at(
        &self,
        tuple: &RelationTuple,
        now: WallTime,
    ) -> Result<bool, AuthzError> {
        let Some(caveat) = &tuple.caveat else {
            return Ok(true);
        };
        let now = plausible_now(now)?;
        self.caveat_holds(caveat, &tuple.object.namespace, now)
    }

    fn caveat_holds(
        &self,
        caveat: &Caveat,
        object_namespace: &NamespaceName,
        now: i64,
    ) -> Result<bool, AuthzError> {
        let namespace = self.schema.namespace(object_namespace).ok_or_else(|| {
            AuthzError::UndefinedNamespace {
                namespace: object_namespace.to_string(),
            }
        })?;
        let predicate =
            namespace
                .caveats
                .get(&caveat.name)
                .ok_or_else(|| AuthzError::UndefinedCaveat {
                    namespace: object_namespace.to_string(),
                    caveat: caveat.name.to_string(),
                })?;
        let (bound, replaced_stored_now) = caveat.context.at(now);
        if replaced_stored_now {
            warn!(
                namespace = %object_namespace,
                caveat = %caveat.name,
                "a caveated tuple stores an argument named `now`; the evaluation's instant stands"
            );
        }
        caveat_eval::evaluate(predicate, &bound)
    }

    /// Expands an `object#relation` into its effective subject set as a
    /// [`UsersetTree`].
    ///
    /// Stored usersets are left unexpanded, and a caveated edge appears only while its caveat
    /// holds. Bounded as a check is.
    ///
    /// # Errors
    ///
    /// [`AuthzError::UndefinedNamespace`] / [`AuthzError::UndefinedRelation`] for an unknown
    /// object type or relation; [`AuthzError::ImplausibleClock`] if `now` is not plausible;
    /// [`AuthzError::CaveatEval`] for a malformed caveat; or a bound or storage error.
    #[instrument(skip(self), fields(object = %object, relation = %relation))]
    pub async fn expand(
        &self,
        object: ObjectRef,
        relation: RelationName,
        consistency: Consistency,
    ) -> Result<UsersetTree, AuthzError> {
        self.store
            .evaluate(consistency, |scope| {
                let (object, relation) = (object.clone(), relation.clone());
                Box::pin(async move { self.expand_in(object, relation, &scope).await })
            })
            .await
    }

    /// Expands inside an already-open read scope.
    ///
    /// # Errors
    ///
    /// Same as [`expand`](Engine::expand).
    pub async fn expand_in(
        &self,
        object: ObjectRef,
        relation: RelationName,
        scope: &S::Scope,
    ) -> Result<UsersetTree, AuthzError> {
        let namespace = self.schema.namespace(&object.namespace).ok_or_else(|| {
            AuthzError::UndefinedNamespace {
                namespace: object.namespace.to_string(),
            }
        })?;
        if namespace.relation(&relation).is_none() {
            return Err(AuthzError::UndefinedRelation {
                namespace: object.namespace.to_string(),
                relation: relation.to_string(),
            });
        }
        let ctx = EvalContext::new(self.limits, scope)?;
        self.expand_relation(&object, &relation, 0, &ctx).await
    }

    async fn expand_relation(
        &self,
        object: &ObjectRef,
        relation: &RelationName,
        depth: usize,
        ctx: &EvalContext<'_, S::Scope>,
    ) -> Result<UsersetTree, AuthzError> {
        if depth > ctx.limits().max_depth {
            return Err(AuthzError::DepthExceeded {
                depth: ctx.limits().max_depth,
            });
        }
        let Some(namespace) = self.schema.namespace(&object.namespace) else {
            return Ok(UsersetTree::Leaf(Vec::new()));
        };
        let Some(relation_ir) = namespace.relation(relation) else {
            return Ok(UsersetTree::Leaf(Vec::new()));
        };
        self.expand_rewrite(object, relation, &relation_ir.rewrite, depth, ctx)
            .await
    }

    #[expect(
        clippy::too_many_arguments,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn expand_rewrite<'a>(
        &'a self,
        object: &'a ObjectRef,
        relation: &'a RelationName,
        rewrite: &'a RewriteIr,
        depth: usize,
        ctx: &'a EvalContext<'a, S::Scope>,
    ) -> TreeFuture<'a> {
        Box::pin(async move {
            match rewrite {
                RewriteIr::This => {
                    ctx.charge_read()?;
                    let edges = self
                        .store
                        .read_all(ctx.scope(), object, relation, ctx.limits().page_limit)
                        .await?;
                    let granting = self.granting_edges(edges, &object.namespace, ctx)?;
                    Ok(UsersetTree::Leaf(
                        granting.into_iter().map(|t| t.subject).collect(),
                    ))
                }
                RewriteIr::ComputedUserset { relation: computed } => {
                    self.expand_relation(object, computed, depth + 1, ctx).await
                }
                RewriteIr::TupleToUserset { tupleset, computed } => {
                    ctx.charge_read()?;
                    let edges = self
                        .store
                        .read_all(ctx.scope(), object, tupleset, ctx.limits().page_limit)
                        .await?;
                    let edges = self.granting_edges(edges, &object.namespace, ctx)?;
                    let mut children = Vec::with_capacity(edges.len());
                    for edge in edges {
                        children.push(
                            self.expand_relation(&edge.subject.object, computed, depth + 1, ctx)
                                .await?,
                        );
                    }
                    Ok(UsersetTree::Union(children))
                }
                RewriteIr::Union(nodes) => Ok(UsersetTree::Union(
                    self.expand_children(object, relation, nodes, depth, ctx)
                        .await?,
                )),
                RewriteIr::Intersection(nodes) => Ok(UsersetTree::Intersection(
                    self.expand_children(object, relation, nodes, depth, ctx)
                        .await?,
                )),
                RewriteIr::Exclusion { include, exclude } => {
                    let include = self
                        .expand_rewrite(object, relation, include, depth, ctx)
                        .await?;
                    let exclude = self
                        .expand_rewrite(object, relation, exclude, depth, ctx)
                        .await?;
                    Ok(UsersetTree::Exclusion {
                        include: Box::new(include),
                        exclude: Box::new(exclude),
                    })
                }
            }
        })
    }

    /// The edges that grant now: every unconditional one, and each caveated one whose caveat holds.
    fn granting_edges(
        &self,
        edges: Vec<RelationTuple>,
        object_namespace: &NamespaceName,
        ctx: &EvalContext<'_, S::Scope>,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let mut granting = Vec::with_capacity(edges.len());
        for edge in edges {
            if let Some(caveat) = &edge.caveat
                && !self.caveat_grants(caveat, object_namespace, ctx)?
            {
                continue;
            }
            granting.push(edge);
        }
        Ok(granting)
    }

    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn expand_children<'a>(
        &'a self,
        object: &'a ObjectRef,
        relation: &'a RelationName,
        nodes: &'a [RewriteIr],
        depth: usize,
        ctx: &'a EvalContext<'a, S::Scope>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<UsersetTree>, AuthzError>> + Send + 'a>> {
        Box::pin(async move {
            let mut children = Vec::with_capacity(nodes.len());
            for node in nodes {
                children.push(
                    self.expand_rewrite(object, relation, node, depth, ctx)
                        .await?,
                );
            }
            Ok(children)
        })
    }

    /// Lists the objects of `object_namespace` on which `subject` holds `relation`.
    ///
    /// Reverse-walks the tuple graph from the subject for a bounded candidate set, then
    /// forward-checks each with [`check`](Engine::check), so the whole algebra is honoured.
    /// Results are capped at `limit`.
    ///
    /// # Errors
    ///
    /// [`AuthzError::RequestCaveatParameter`] if `context` binds any parameter;
    /// [`AuthzError::ImplausibleClock`] if `now` is not plausible;
    /// [`AuthzError::UndefinedNamespace`] / [`AuthzError::UndefinedRelation`] for an unknown
    /// target type or relation; or a bound or storage error.
    #[instrument(skip(self, context), fields(subject = %subject, object_namespace = %object_namespace, relation = %relation, limit))]
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub async fn list_objects(
        &self,
        subject: SubjectRef,
        object_namespace: NamespaceName,
        relation: RelationName,
        context: CaveatContext,
        consistency: Consistency,
        limit: usize,
    ) -> Result<Vec<ObjectRef>, AuthzError> {
        self.store
            .evaluate(consistency, |scope| {
                let (subject, object_namespace, relation, context) = (
                    subject.clone(),
                    object_namespace.clone(),
                    relation.clone(),
                    context.clone(),
                );
                Box::pin(async move {
                    self.list_objects_in(
                        subject,
                        object_namespace,
                        relation,
                        context,
                        &scope,
                        limit,
                    )
                    .await
                })
            })
            .await
    }

    /// Lists objects inside an already-open read scope.
    ///
    /// # Errors
    ///
    /// Same as [`list_objects`](Engine::list_objects).
    #[expect(
        clippy::too_many_arguments,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub async fn list_objects_in(
        &self,
        subject: SubjectRef,
        object_namespace: NamespaceName,
        relation: RelationName,
        context: CaveatContext,
        scope: &S::Scope,
        limit: usize,
    ) -> Result<Vec<ObjectRef>, AuthzError> {
        let namespace = self.schema.namespace(&object_namespace).ok_or_else(|| {
            AuthzError::UndefinedNamespace {
                namespace: object_namespace.to_string(),
            }
        })?;
        if namespace.relation(&relation).is_none() {
            return Err(AuthzError::UndefinedRelation {
                namespace: object_namespace.to_string(),
                relation: relation.to_string(),
            });
        }

        // One snapshot, budget and memo for the reverse walk and every forward re-check.
        refuse_request_context(&context)?;
        let ctx = EvalContext::<_, ()>::new(self.limits, scope)?;

        // Reverse BFS from the subject to every object it is transitively a subject of.
        let mut frontier: Vec<SubjectRef> = vec![subject.clone()];
        let mut visited: HashSet<SubjectRef> = HashSet::new();
        let mut candidates: BTreeSet<ObjectRef> = BTreeSet::new();

        while let Some(current) = frontier.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            ctx.charge_read()?;
            let tuples = self
                .store
                .read_by_subject(ctx.scope(), &current, None, None, ctx.limits().page_limit)
                .await?;
            for tuple in tuples {
                if tuple.object.namespace == object_namespace {
                    candidates.insert(tuple.object.clone());
                }
                let userset = SubjectRef {
                    object: tuple.object,
                    relation: Some(tuple.relation),
                };
                if !visited.contains(&userset) {
                    frontier.push(userset);
                }
            }
        }

        // Forward-confirm each candidate through the same recursion.
        let walk = CheckWalk {
            subject: &subject,
            ctx: &ctx,
        };
        let mut result = Vec::new();
        for candidate in candidates {
            if result.len() >= limit {
                break;
            }
            if self
                .check_inner(&candidate, &relation, 0, CheckPath::root(), walk)
                .await?
                .is_some()
            {
                result.push(candidate);
            }
        }
        Ok(result)
    }
}

/// The stored tuple a point lookup found: the edge it asked for, with the caveat it read.
fn stored(
    object: &ObjectRef,
    relation: &RelationName,
    subject: &SubjectRef,
    edge: DirectEdge,
) -> RelationTuple {
    let tuple = RelationTuple::new(object.clone(), relation.clone(), subject.clone());
    match edge {
        DirectEdge::Unconditional => tuple,
        DirectEdge::Conditional(caveat) => tuple.with_caveat(caveat),
    }
}

/// A branch reached through a stored `tuple`: its allow rests on the tuple as well.
fn through<'a, J: Justification>(
    tuple: &'a RelationTuple,
    branch: OutcomeFuture<'a, J>,
) -> OutcomeFuture<'a, J> {
    Box::pin(async move {
        Ok(branch
            .await?
            .map(|justification| J::edge(|| tuple.clone()).join(justification)))
    })
}

/// Whether a set index at `watermark` may answer a check requesting `consistency`.
///
/// Falls back to the authoritative engine whenever in doubt.
fn index_fresh_enough(consistency: &Consistency, watermark: HybridTs) -> bool {
    match consistency {
        Consistency::MinimizeLatency => true,
        Consistency::AtLeastAsFresh(zookie) => watermark >= zookie.ts(),
        Consistency::FullConsistency => false,
    }
}
