//! The [`Authorizer`]: the framework's permission API, its in-process [`NativeEngine`], the
//! object-safe [`DynAuthorizer`] and the [`HasAuthorizer`] seam.

#[allow(clippy::module_inception)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
mod authorizer;
mod authorizer_ext;
mod check_builder;
mod check_response;
mod dyn_authorizer;
mod expand_response;
mod has_authorizer;
mod list_builder;
mod list_objects_response;
#[cfg(feature = "sql")]
mod locked_decision;
#[cfg(feature = "sql")]
mod locked_questions;
mod native_engine;
#[cfg(feature = "sql")]
mod transactional_authorizer;

pub use authorizer::Authorizer;
pub use authorizer_ext::AuthorizerExt;
pub use check_builder::CheckBuilder;
pub use check_response::CheckResponse;
pub use dyn_authorizer::DynAuthorizer;
pub use expand_response::ExpandResponse;
pub use has_authorizer::HasAuthorizer;
pub use list_builder::ListBuilder;
pub use list_objects_response::ListObjectsResponse;
#[cfg(feature = "sql")]
pub use locked_decision::LockedDecision;
#[cfg(feature = "sql")]
pub use locked_questions::LockedQuestions;
pub use native_engine::NativeEngine;
#[cfg(feature = "sql")]
pub use transactional_authorizer::TransactionalAuthorizer;
