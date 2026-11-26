//! The handler-facing connection context.

pub(crate) mod connection_state;
mod ws_context;

pub use ws_context::WsContext;
#[cfg(test)]
pub(crate) use ws_context::test_support;
