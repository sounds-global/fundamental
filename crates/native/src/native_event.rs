use tauri::{RunEvent, WindowEvent};

use crate::WindowEventKind;

/// A lifecycle event of the running native app.
///
/// `#[non_exhaustive]`; platform-specific variants exist only where they can occur.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeEvent {
    /// The app finished launching and the run loop is live.
    Ready,
    /// The user or OS asked the app to exit; a hook may still veto it.
    ExitRequested {
        /// The requested exit code: `None` for a user-initiated exit.
        code: Option<i32>,
    },
    /// The app is exiting; last chance to flush state.
    Exit,
    /// Something happened to one of the app's windows.
    Window {
        /// The window's label (`"main"` for the primary window).
        label: String,
        /// What happened.
        event: WindowEventKind,
    },
    /// The app moved to the background (mobile: `onPause` /
    /// `applicationWillResignActive`).
    #[cfg(mobile)]
    Suspended,
    /// The app returned to the foreground (mobile: `onResume` /
    /// `applicationWillEnterForeground`).
    #[cfg(mobile)]
    Resumed,
    /// The OS asked the app to open resources such as deep links, files or URL schemes,
    /// including the ones that launched it.
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        feature = "deep-link"
    ))]
    Opened {
        /// The requested URLs, verbatim as the OS delivered them.
        urls: Vec<String>,
    },
    /// The user re-activated the app (macOS: dock-icon click).
    #[cfg(target_os = "macos")]
    Reopen {
        /// Whether the app already had a visible window.
        has_visible_windows: bool,
    },
    /// A menu item was activated; it is also re-emitted to the webview.
    #[cfg(desktop)]
    MenuSelected {
        /// The activated item's id.
        id: String,
    },
    /// The device's push registration is live end to end: the registrar accepted the token.
    #[cfg(feature = "push")]
    PushRegistered {
        /// The service that issued the token.
        provider: crate::PushProvider,
    },
    /// The device's push registration is gone, or was never obtained.
    #[cfg(feature = "push")]
    PushUnregistered {
        /// Why delivery stopped.
        reason: crate::PushUnregistered,
    },
    /// A push notification arrived.
    ///
    /// Delivered synchronously: the app is suspended once the hook returns.
    #[cfg(feature = "push")]
    PushReceived {
        /// The notification.
        message: crate::PushMessage,
    },
    /// The user's notification authorization changed, or was read at startup.
    #[cfg(feature = "push")]
    PushAuthorization {
        /// The platform's current answer.
        status: crate::PushAuthorization,
    },
}

impl NativeEvent {
    /// Whether this is an [`Opened`](Self::Opened) event (a deep link / URL open).
    ///
    /// Lets the builder drop the raw `Opened` when the deep-link plugin delivers it.
    pub(crate) fn is_opened(&self) -> bool {
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "android",
            feature = "deep-link"
        ))]
        {
            matches!(self, Self::Opened { .. })
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "android",
            feature = "deep-link"
        )))]
        {
            false
        }
    }

    /// Whether this is a moment worth retrying a failed push registration: launch or return to
    /// the foreground.
    #[cfg(feature = "push")]
    pub(crate) fn is_push_retry_moment(&self) -> bool {
        #[cfg(mobile)]
        {
            matches!(self, Self::Ready | Self::Resumed)
        }
        #[cfg(not(mobile))]
        {
            matches!(self, Self::Ready)
        }
    }

    /// Maps a run-loop event to its hook-visible moment, if it has one.
    pub(crate) fn from_run_event(event: &RunEvent) -> Option<Self> {
        match event {
            RunEvent::Ready => Some(Self::Ready),
            RunEvent::ExitRequested { code, .. } => Some(Self::ExitRequested { code: *code }),
            RunEvent::Exit => Some(Self::Exit),
            RunEvent::WindowEvent { label, event, .. } => Self::from_window_event(label, event),
            #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
            RunEvent::Opened { urls } => Some(Self::Opened {
                urls: urls.iter().map(ToString::to_string).collect(),
            }),
            #[cfg(target_os = "macos")]
            RunEvent::Reopen {
                has_visible_windows,
                ..
            } => Some(Self::Reopen {
                has_visible_windows: *has_visible_windows,
            }),
            #[cfg(desktop)]
            RunEvent::MenuEvent(event) => Some(Self::MenuSelected {
                id: event.id().as_ref().to_owned(),
            }),
            _ => None,
        }
    }

    /// Maps one window's runtime event; on mobile suspend and resume arrive as window events.
    pub(crate) fn from_window_event(label: &str, event: &WindowEvent) -> Option<Self> {
        #[cfg(mobile)]
        match event {
            WindowEvent::Suspended => return Some(Self::Suspended),
            WindowEvent::Resumed => return Some(Self::Resumed),
            _ => {}
        }
        WindowEventKind::from_tauri(event).map(|kind| Self::Window {
            label: label.to_owned(),
            event: kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use tauri::{RunEvent, WindowEvent};

    use super::NativeEvent;
    use crate::WindowEventKind;

    #[test]
    fn lifecycle_moments_map() {
        assert_eq!(
            NativeEvent::from_run_event(&RunEvent::Ready),
            Some(NativeEvent::Ready)
        );
        assert_eq!(
            NativeEvent::from_run_event(&RunEvent::Exit),
            Some(NativeEvent::Exit)
        );
    }

    #[test]
    fn internal_run_loop_events_are_not_surfaced() {
        assert_eq!(
            NativeEvent::from_run_event(&RunEvent::MainEventsCleared),
            None
        );
    }

    #[test]
    fn window_events_carry_their_label_and_kind() {
        let event = NativeEvent::from_window_event("settings", &WindowEvent::Focused(true));
        assert_eq!(
            event,
            Some(NativeEvent::Window {
                label: "settings".to_owned(),
                event: WindowEventKind::Focused(true),
            })
        );
    }

    #[test]
    fn unmapped_window_events_yield_nothing() {
        let resized = WindowEvent::Resized(tauri::PhysicalSize::new(800, 600));
        assert_eq!(NativeEvent::from_window_event("main", &resized), None);
    }

    #[test]
    fn non_opened_events_are_not_opened() {
        assert!(!NativeEvent::Ready.is_opened());
        assert!(!NativeEvent::Exit.is_opened());
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        feature = "deep-link"
    ))]
    #[test]
    fn opened_events_are_recognized() {
        let event = NativeEvent::Opened {
            urls: vec!["openid4vp://authorize?x=1".to_owned()],
        };
        assert!(event.is_opened());
    }

    #[cfg(desktop)]
    #[test]
    fn menu_activation_maps_to_its_id() {
        let event = tauri::menu::MenuEvent {
            id: tauri::menu::MenuId::new("preferences"),
        };
        assert_eq!(
            NativeEvent::from_run_event(&RunEvent::MenuEvent(event)),
            Some(NativeEvent::MenuSelected {
                id: "preferences".to_owned(),
            })
        );
    }
}
