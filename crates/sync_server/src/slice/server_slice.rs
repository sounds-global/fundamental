use std::future::Future;

use fundamental_db::Filter;
use fundamental_sync::Slice;

use crate::slice::{SliceContext, SliceError};

/// The server-only half of a [`Slice`]: the trusted predicate that scopes it to the
/// authenticated principal.
///
/// `#[slice]` emits it only off wasm, so the resolver never reaches a device build.
pub trait ServerSlice: Slice {
    /// Resolves a subscription for the principal in `ctx` and the validated `params` into the
    /// [`Filter`] that scopes the slice.
    ///
    /// # Errors
    /// Returns a [`SliceError`] when the principal is not authorized or the predicate fails.
    fn resolve(
        ctx: SliceContext,
        params: Self::Params,
    ) -> impl Future<Output = Result<Filter, SliceError>> + Send;
}
