use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tracing::instrument;
#[cfg(feature = "push")]
use tracing::{debug, info, warn};

use crate::{ActionBridge, NativeError, WindowConfig};
#[cfg(feature = "push")]
use crate::{
    NativeEvent, PushAuthorization, PushMessage, PushRegistration, PushUnregistered, push,
};

/// A handle to the running app: the bridge, events, and window control.
///
/// Cheap to clone; [`tauri`](Self::tauri) is the escape hatch to the raw `AppHandle`.
#[derive(Clone)]
pub struct NativeHandle {
    app: AppHandle,
    bridge: Arc<dyn ActionBridge>,
}

impl NativeHandle {
    /// Wraps a running app and its managed bridge.
    pub(crate) fn new(app: AppHandle, bridge: Arc<dyn ActionBridge>) -> Self {
        Self { app, bridge }
    }

    /// Recovers the framework handle from a raw Tauri one, the inverse of [`tauri`](Self::tauri).
    ///
    /// `None` for a Tauri app not hosted by [`NativeBuilder`](crate::NativeBuilder).
    #[must_use]
    pub fn from_tauri(app: &AppHandle) -> Option<Self> {
        let bridge = app.try_state::<crate::invoke::BridgeState>()?;
        Some(Self::new(app.clone(), Arc::clone(&bridge.0)))
    }

    /// The app's action bridge, for platform code that runs actions itself.
    #[must_use]
    pub fn bridge(&self) -> Arc<dyn ActionBridge> {
        Arc::clone(&self.bridge)
    }

    /// The underlying Tauri handle — the escape hatch for OS integrations.
    #[must_use]
    pub fn tauri(&self) -> &AppHandle {
        &self.app
    }

    /// Emits `event` with a JSON `payload` to every webview.
    ///
    /// # Errors
    /// [`NativeError::Emit`] when serialization or delivery fails.
    #[instrument(skip(self, payload))]
    pub fn emit(&self, event: &str, payload: impl Serialize + Clone) -> Result<(), NativeError> {
        self.app
            .emit(event, payload)
            .map_err(|source| NativeError::Emit {
                event: event.to_owned(),
                source,
            })
    }

    /// Exits the app with `code`.
    pub fn exit(&self, code: i32) {
        self.app.exit(code);
    }

    /// Opens `url` with the OS's default handler (the `deep-link` feature).
    ///
    /// The URL is never logged; it may carry a secret query.
    ///
    /// # Errors
    /// [`NativeError::DeepLinkOpen`] when the OS has no handler for the URL or
    /// the open is refused.
    #[cfg(feature = "deep-link")]
    #[instrument(skip(self, url))]
    pub fn open_url(&self, url: &str) -> Result<(), NativeError> {
        use tauri_plugin_opener::OpenerExt;
        self.app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|source| NativeError::DeepLinkOpen {
                detail: source.to_string(),
            })
    }

    /// The platform issued a push registration token (first run, or a rotation).
    ///
    /// A token already held is dropped, a rotation goes through, and a registration is kept only
    /// once the registrar accepts it, so a failure is retried later. The token is never logged.
    ///
    /// # Errors
    /// [`NativeError::PushInactive`] when the app never called
    /// [`NativeBuilder::push_notifications`](crate::NativeBuilder::push_notifications).
    #[cfg(feature = "push")]
    #[instrument(skip(self, registration), fields(provider = ?registration.provider()))]
    pub fn push_registered(&self, registration: PushRegistration) -> Result<(), NativeError> {
        let inner = self.push_seam()?;
        self.deliver_registration(&inner, registration);
        Ok(())
    }

    /// The device's push registration was revoked, or could not be obtained.
    ///
    /// Forgets the registration, including one still being recorded, tells the registrar, and
    /// reports it to the hooks.
    ///
    /// # Errors
    /// [`NativeError::PushInactive`] when push was never activated.
    #[cfg(feature = "push")]
    #[instrument(skip(self, reason), fields(reason = ?reason))]
    pub fn push_unregistered(&self, reason: PushUnregistered) -> Result<(), NativeError> {
        let inner = self.push_seam()?;
        inner.forget();
        inner.registrar.unregistered(reason.clone());
        push::dispatch(
            self,
            &inner.hooks,
            &NativeEvent::PushUnregistered { reason },
        );
        Ok(())
    }

    /// A push notification arrived.
    ///
    /// Delivered synchronously to the hooks, then to the webview as `fundamental:push`: the app
    /// is suspended once it returns. One that arrives before the hooks start is held. The payload
    /// is never logged.
    ///
    /// # Errors
    /// [`NativeError::PushInactive`] when push was never activated.
    #[cfg(feature = "push")]
    #[instrument(skip(self, message), fields(delivery = message.delivery().label()))]
    pub fn push_received(&self, message: PushMessage) -> Result<(), NativeError> {
        let inner = self.push_seam()?;
        push::settle(self, &inner.hooks, inner.offer_message(message));
        Ok(())
    }

    /// The user's notification authorization was read, or changed.
    ///
    /// # Errors
    /// [`NativeError::PushInactive`] when push was never activated.
    #[cfg(feature = "push")]
    #[instrument(skip(self, status), fields(status = ?status))]
    pub fn push_authorization(&self, status: PushAuthorization) -> Result<(), NativeError> {
        let inner = self.push_seam()?;
        push::dispatch(
            self,
            &inner.hooks,
            &NativeEvent::PushAuthorization { status },
        );
        Ok(())
    }

    #[cfg(feature = "push")]
    fn push_seam(&self) -> Result<Arc<push::PushInner>, NativeError> {
        self.app
            .try_state::<push::PushState>()
            .map(|state| Arc::clone(&state.0))
            .ok_or(NativeError::PushInactive)
    }

    #[cfg(feature = "push")]
    fn deliver_registration(&self, inner: &Arc<push::PushInner>, registration: PushRegistration) {
        let push::PushDecision::Deliver { epoch } = inner.offer(&registration) else {
            debug!("the push registration is unchanged; not registered again");
            return;
        };
        let provider = registration.provider();
        let handle = self.clone();
        let inner = Arc::clone(inner);
        tauri::async_runtime::spawn(async move {
            let outcome = inner.registrar.register(registration).await;
            // Settled before dispatching: a hook may call back in.
            inner.settle(epoch, outcome.is_ok());
            match outcome {
                Ok(()) => {
                    info!(?provider, "the device's push registration is live");
                    push::dispatch(
                        &handle,
                        &inner.hooks,
                        &NativeEvent::PushRegistered { provider },
                    );
                }
                Err(error) => {
                    warn!(
                        error = ?error,
                        ?provider,
                        "the push registrar refused the token; it will be offered again"
                    );
                    let reason = PushUnregistered::Failed {
                        detail: error.to_string(),
                    };
                    push::dispatch(
                        &handle,
                        &inner.hooks,
                        &NativeEvent::PushUnregistered { reason },
                    );
                }
            }
        });
    }

    /// Offers a registration the registrar refused, if one is waiting.
    ///
    /// Called at launch and on a return to the foreground.
    #[cfg(feature = "push")]
    pub(crate) fn retry_push_registration(&self) {
        let Ok(inner) = self.push_seam() else {
            return;
        };
        let Some(registration) = inner.retryable() else {
            return;
        };
        info!("retrying a push registration the registrar refused");
        self.deliver_registration(&inner, registration);
    }

    /// Creates a window from `config` at runtime.
    ///
    /// # Errors
    /// [`NativeError::Window`] when the platform refuses the window.
    #[instrument(skip(self, config), fields(label = %config.label))]
    pub fn create_window(&self, config: &WindowConfig) -> Result<(), NativeError> {
        config.create(&self.app, &[])
    }

    /// Shows and focuses the window labeled `label`.
    ///
    /// # Errors
    /// [`NativeError::WindowNotFound`] / [`NativeError::Window`].
    pub fn show_window(&self, label: &str) -> Result<(), NativeError> {
        let window = self.window(label)?;
        self.window_op(label, window.show())?;
        self.window_op(label, window.set_focus())
    }

    /// Hides the window labeled `label`.
    ///
    /// # Errors
    /// [`NativeError::WindowNotFound`] / [`NativeError::Window`].
    pub fn hide_window(&self, label: &str) -> Result<(), NativeError> {
        let window = self.window(label)?;
        self.window_op(label, window.hide())
    }

    /// Focuses the window labeled `label`.
    ///
    /// # Errors
    /// [`NativeError::WindowNotFound`] / [`NativeError::Window`].
    pub fn focus_window(&self, label: &str) -> Result<(), NativeError> {
        let window = self.window(label)?;
        self.window_op(label, window.set_focus())
    }

    /// Closes the window labeled `label`.
    ///
    /// # Errors
    /// [`NativeError::WindowNotFound`] / [`NativeError::Window`].
    pub fn close_window(&self, label: &str) -> Result<(), NativeError> {
        let window = self.window(label)?;
        self.window_op(label, window.close())
    }

    /// Minimizes the window labeled `label`.
    ///
    /// # Errors
    /// [`NativeError::WindowNotFound`] / [`NativeError::Window`].
    pub fn minimize_window(&self, label: &str) -> Result<(), NativeError> {
        let window = self.window(label)?;
        self.window_op(label, window.minimize())
    }

    /// Maximizes the window labeled `label`.
    ///
    /// # Errors
    /// [`NativeError::WindowNotFound`] / [`NativeError::Window`].
    pub fn maximize_window(&self, label: &str) -> Result<(), NativeError> {
        let window = self.window(label)?;
        self.window_op(label, window.maximize())
    }

    /// Toggles fullscreen on the window labeled `label`.
    ///
    /// # Errors
    /// [`NativeError::WindowNotFound`] / [`NativeError::Window`].
    pub fn toggle_fullscreen(&self, label: &str) -> Result<(), NativeError> {
        let window = self.window(label)?;
        let fullscreen = self.window_value(label, window.is_fullscreen())?;
        self.window_op(label, window.set_fullscreen(!fullscreen))
    }

    fn window(&self, label: &str) -> Result<WebviewWindow, NativeError> {
        self.app
            .get_webview_window(label)
            .ok_or_else(|| NativeError::WindowNotFound {
                label: label.to_owned(),
            })
    }

    fn window_op(&self, label: &str, result: tauri::Result<()>) -> Result<(), NativeError> {
        result.map_err(|source| NativeError::Window {
            label: label.to_owned(),
            source,
        })
    }

    fn window_value<T>(&self, label: &str, result: tauri::Result<T>) -> Result<T, NativeError> {
        result.map_err(|source| NativeError::Window {
            label: label.to_owned(),
            source,
        })
    }
}

impl std::fmt::Debug for NativeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeHandle").finish_non_exhaustive()
    }
}
