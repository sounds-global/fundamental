//! The `authorize(...)` HTTP permission guard (`server` feature): [`authorize`] builds a
//! [`PermissionGuard`] for an `#[action]`, which resolves its object through an
//! [`ObjectSource`] and its subject through a [`SubjectSource`].

mod any_relation;
mod authorized;
mod forbidden;
mod guard_check;
mod guard_decision;
mod object_source;
mod permission_guard;
mod subject_source;

pub use any_relation::AnyRelation;
pub use authorized::Authorized;
pub use forbidden::Forbidden;
pub use guard_check::GuardCheck;
pub use guard_decision::GuardDecision;
pub use object_source::{ConstObject, ObjectSource, PathObject, StateObject};
pub use permission_guard::{PermissionGuard, authorize};
pub use subject_source::{PrincipalSubject, ResolvedSubject, SubjectSource};
