//! Portable client-call shape metadata for mounted actions.

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
use core::future::Future;
#[cfg(feature = "std")]
use std::borrow::Cow;

use crate::AnyError;
use crate::action::ActionPath;

/// The transport a surface uses to call an action over its own wire.
pub trait ActionTransport {
    /// The future returned by this transport for an output type.
    type Future<O>: Future<Output = Result<O, AnyError>> + 'static
    where
        O: 'static;

    /// Sends one typed request to the action's method and path.
    ///
    /// A transport refuses a `path` that still holds a `{capture}`.
    fn call<I, O>(method: &'static str, path: Cow<'static, str>, input: I) -> Self::Future<O>
    where
        I: serde::Serialize + 'static,
        O: serde::de::DeserializeOwned + 'static;
}

/// Describes how to call an action without exposing its handler's containing module.
pub trait ClientAction: crate::action::Action {
    /// The semantic role assigned to this action for generated reactive hooks.
    type Operation: crate::action::Operation;

    /// The callable shape for this action on transport `T`.
    type Function<T: ActionTransport>;

    /// The successful result decoded from the action response.
    type Success: 'static;

    /// Calls the action at its declared path with its typed input through transport `T`.
    ///
    /// An action whose path holds captures is called with [`call_at`](Self::call_at).
    fn call<T: ActionTransport>(input: Self::Input) -> T::Future<Self::Success>;

    /// Calls the action at `path`, its captures filled by the marker's `path(..)`, through transport `T`.
    fn call_at<T: ActionTransport>(
        path: ActionPath<Self>,
        input: Self::Input,
    ) -> T::Future<Self::Success>
    where
        Self: Sized;
}
