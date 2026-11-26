//! Relation tuples: the backend-neutral relationship model, and the [`TupleStore`] seam each
//! storage backend implements.

mod canonical_id;
mod caveat;
mod caveat_context;
mod clock_scope;
mod direct_edge;
mod into_canonical_id;
mod into_subject;
mod object_ref;
mod read_scope;
mod relation_tuple;
mod scoped_eval;
mod subject_ref;
mod tuple_key;
mod tuple_store;
mod wall_time;

pub use canonical_id::{CanonicalId, WILDCARD};
pub use caveat::Caveat;
pub use caveat_context::CaveatContext;
pub use clock_scope::ClockScope;
pub use direct_edge::DirectEdge;
pub use into_canonical_id::IntoCanonicalId;
pub use into_subject::IntoSubject;
pub use object_ref::ObjectRef;
pub use read_scope::ReadScope;
pub use relation_tuple::RelationTuple;
pub use scoped_eval::ScopedEval;
pub use subject_ref::SubjectRef;
pub use tuple_key::TupleKey;
pub use tuple_store::TupleStore;
pub use wall_time::WallTime;
