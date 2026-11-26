use fundamental_core::AnyError;

use crate::{ExitAction, NativeEvent, NativeHandle};

/// Hand-written platform code, hooked into the app's lifecycle.
///
/// A scaffolded app has one `platforms/<os>/hooks.rs` per OS; everything a hook reaches comes
/// through the [`NativeHandle`].
pub trait PlatformHooks: Send + Sync + 'static {
    /// The hook's name, carried in setup-failure errors and spans.
    fn name(&self) -> &'static str;

    /// Runs once at startup, after windows exist and before the run loop.
    ///
    /// # Errors
    /// A returned error aborts startup (`NATIVE_010`); a hook that can degrade should log and
    /// return `Ok`.
    fn on_setup(&self, app: &NativeHandle) -> Result<(), AnyError> {
        let _ = app;
        Ok(())
    }

    /// Observes one lifecycle [`NativeEvent`].
    fn on_event(&self, app: &NativeHandle, event: &NativeEvent) {
        let _ = (app, event);
    }

    /// Decides whether a user-initiated exit request may proceed.
    ///
    /// Return [`ExitAction::Prevent`] to keep the app running. The first `Prevent` in registration
    /// order wins; programmatic exits always proceed.
    fn on_exit_requested(&self, app: &NativeHandle, code: Option<i32>) -> ExitAction {
        let _ = (app, code);
        ExitAction::Exit
    }
}

/// The no-op hooks: a platform with nothing to add returns `()`.
impl PlatformHooks for () {
    fn name(&self) -> &'static str {
        "none"
    }
}
