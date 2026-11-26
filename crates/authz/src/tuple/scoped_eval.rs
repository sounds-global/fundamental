//! [`ScopedEval`]: the future an evaluation body returns.

use std::future::Future;
use std::pin::Pin;

use crate::error::AuthzError;

/// The boxed, explicitly-`Send` future a
/// [`TupleStore::evaluate`](super::TupleStore::evaluate) body returns.
///
///
/// Boxed and `Send`, because an inferred `Send` does not survive the generic `S: TupleStore`
/// boundary. `'a` is the borrow of the store; the body owns its scope as an `Arc`.
pub type ScopedEval<'a, T> = Pin<Box<dyn Future<Output = Result<T, AuthzError>> + Send + 'a>>;
