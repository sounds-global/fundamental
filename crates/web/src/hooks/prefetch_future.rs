use std::future::Future;

/// Wraps browser futures for Leptos's `Send`-bounded `Resource` constructor.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub fn prefetch_future<F>(future: F) -> send_wrapper::SendWrapper<F>
where
    F: Future,
{
    send_wrapper::SendWrapper::new(future)
}

/// Passes native futures through unchanged.
#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub fn prefetch_future<F>(future: F) -> F
where
    F: Future,
{
    future
}
