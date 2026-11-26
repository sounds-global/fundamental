//! [`EffectModel`]: a model an effect may write.

use fundamental_core::Model;

/// A model whose table belongs to the application, so code lent a transaction may write it.
///
/// `#[derive(Model)]` implements it for every model except the framework's `fundamental_*`
/// tables, so such a handle cannot reach a ledger kept beside the effect.
pub trait EffectModel: Model {}
