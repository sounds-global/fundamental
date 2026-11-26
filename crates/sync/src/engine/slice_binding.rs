//! Binds one engine sync cycle to a slice.

use fundamental_core::model::offline::ConflictStrategy;
use fundamental_core::validate::Validate;
use serde_json::Value;

use crate::{Slice, SliceBindError, Syncable};

/// Ties an engine pull or push to one slice: its name, params, local table and conflict strategy.
#[derive(Debug, Clone)]
pub struct SliceBinding {
    slice: String,
    params: Value,
    local_table: String,
    strategy: ConflictStrategy,
}

impl SliceBinding {
    /// Binds `slice` with `params` to its `local_table`, reconciling pulls with `strategy`.
    pub fn new(
        slice: impl Into<String>,
        local_table: impl Into<String>,
        params: Value,
        strategy: ConflictStrategy,
    ) -> Self {
        Self {
            slice: slice.into(),
            params,
            local_table: local_table.into(),
            strategy,
        }
    }

    /// Binds a typed [`Slice`] `S` from typed `params`, taking the mirror table and strategy from the
    /// model.
    /// # Errors
    /// Returns [`SliceBindError::Invalid`] if `params` fail validation, or
    /// [`SliceBindError::Serialize`] if they cannot be serialized.
    pub fn for_slice<S>(params: S::Params) -> Result<Self, SliceBindError>
    where
        S: Slice,
        S::Model: Syncable,
    {
        params.validate()?;
        let value =
            serde_json::to_value(&params).map_err(|e| SliceBindError::Serialize(e.to_string()))?;
        Ok(Self::new(
            S::NAME,
            <S::Model as Syncable>::MIRROR_TABLE,
            value,
            <S::Model as Syncable>::STRATEGY,
        ))
    }

    /// The server slice name to sync.
    #[must_use]
    pub fn slice(&self) -> &str {
        &self.slice
    }

    /// The slice's validated parameters.
    #[must_use]
    pub fn params(&self) -> &Value {
        &self.params
    }

    /// The local mirror table the slice materializes into.
    #[must_use]
    pub fn local_table(&self) -> &str {
        &self.local_table
    }

    /// The conflict-resolution strategy for reconciling pulled rows.
    #[must_use]
    pub fn strategy(&self) -> ConflictStrategy {
        self.strategy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_carries_the_slice_name_table_params_and_strategy() {
        let binding = SliceBinding::new(
            "mine",
            "widgets",
            serde_json::json!({}),
            ConflictStrategy::LastWriteWins,
        );
        assert_eq!(binding.slice(), "mine");
        assert_eq!(binding.local_table(), "widgets");
        assert_eq!(binding.params(), &serde_json::json!({}));
        assert!(matches!(
            binding.strategy(),
            ConflictStrategy::LastWriteWins
        ));
    }

    // `for_slice` is tested in `fundamental_web`: the derive does not resolve inside this crate.
}
