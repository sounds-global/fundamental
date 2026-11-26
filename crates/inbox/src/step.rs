//! [`Step`] — an inbox handler, as the inbox calls it.

use std::future::Future;
use std::pin::Pin;

use fundamental_sql::EffectTransaction;

use crate::Inbound;

/// The future an inbox handler returns, boxed so one `fn` pointer type fits every handler.
pub type StepFuture<'a, E> = Pin<Box<dyn Future<Output = Result<(), E>> + Send + 'a>>;

/// An inbox handler: a plain `fn` handed the narrowed transaction and the event; being `Copy`
/// lets the transaction body re-run on a conflict.
pub type Step<M, E> =
    for<'a, 't> fn(&'a mut EffectTransaction<'t>, &'a Inbound<M>) -> StepFuture<'a, E>;
