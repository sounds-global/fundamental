//! The Leptos view surface, re-exported as the framework's stable web prelude.

pub use leptos::prelude::*;

#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use crate::{
    ActionHook, AsyncDefaults, AsyncDefaultsConfig, AsyncErrorView, AsyncSuccessView, Mutation,
    MutationScope, Query,
};

/// The live reactive read hooks and their context provider (feature `live`).
#[cfg(feature = "live")]
pub use crate::live::{provide_live, use_live, use_live_one, use_live_with};

#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub use crate::hooks::*;
