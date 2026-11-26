//! [`ReconnectMaxAttemptsIsRemoved`] — the bound that refuses the removed reconnect budget.

/// Implemented by no type, so setting the removed reconnect budget fails to compile.
#[diagnostic::on_unimplemented(
    message = "`reconnect_max_attempts` is removed: a sync session never gives up on its own",
    label = "no value is accepted here",
    note = "watch `SyncClient::status()` and call `SyncClient::shutdown()` to end a session the app no longer wants"
)]
pub trait ReconnectMaxAttemptsIsRemoved: sealed::Never {}

mod sealed {
    /// Implemented by nothing, so nothing outside can implement the removed knob's bound.
    pub trait Never {}
}
