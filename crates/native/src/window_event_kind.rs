use tauri::WindowEvent;

/// A per-window event, observed through
/// [`NativeEvent::Window`](crate::NativeEvent::Window).
///
/// `#[non_exhaustive]`, and observation only: a close cannot be vetoed here.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowEventKind {
    /// The window gained (`true`) or lost (`false`) focus.
    Focused(bool),
    /// The user or OS asked the window to close; the window is still alive.
    CloseRequested,
    /// The window has been destroyed.
    Destroyed,
}

impl WindowEventKind {
    /// Maps a runtime window event to its hook-visible kind, if it has one.
    pub(crate) fn from_tauri(event: &WindowEvent) -> Option<Self> {
        match event {
            WindowEvent::Focused(focused) => Some(Self::Focused(*focused)),
            WindowEvent::CloseRequested { .. } => Some(Self::CloseRequested),
            WindowEvent::Destroyed => Some(Self::Destroyed),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use tauri::{PhysicalSize, WindowEvent};

    use super::WindowEventKind;

    #[test]
    fn focus_maps_with_its_direction() {
        assert_eq!(
            WindowEventKind::from_tauri(&WindowEvent::Focused(true)),
            Some(WindowEventKind::Focused(true))
        );
        assert_eq!(
            WindowEventKind::from_tauri(&WindowEvent::Focused(false)),
            Some(WindowEventKind::Focused(false))
        );
    }

    #[test]
    fn destroyed_maps() {
        assert_eq!(
            WindowEventKind::from_tauri(&WindowEvent::Destroyed),
            Some(WindowEventKind::Destroyed)
        );
    }

    #[test]
    fn noisy_events_are_not_surfaced() {
        // Resize is deliberately unmapped.
        let resized = WindowEvent::Resized(PhysicalSize::new(800, 600));
        assert_eq!(WindowEventKind::from_tauri(&resized), None);
    }
}
