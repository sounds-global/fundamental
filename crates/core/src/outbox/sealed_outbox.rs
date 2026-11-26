//! [`SealedOutbox`] — a model whose every row carries a seal the framework writes and checks.

use crate::model::Model;

/// A model declared `#[model(outbox)]`, so its `Model` derive seals every row it writes and
/// checks the seal on every row it reads.
///
/// Implement it only through `#[derive(Model)]` with `#[model(outbox)]`, which computes the seal.
pub trait SealedOutbox: Model {}
