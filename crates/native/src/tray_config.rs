use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, Submenu, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, Runtime};
use tracing::{debug, warn};

use crate::NativeError;

/// The app's system-tray icon and menu.
///
/// The reserved ids `show`, `hide` and `quit` act on the main window and app; every other id
/// re-emits to the webview as `tray:{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrayConfig {
    /// Hover tooltip, if any.
    pub tooltip: Option<String>,
    /// The tray menu, top to bottom.
    pub menu_items: Vec<TrayMenuItem>,
    /// Whether a left click shows and focuses the main window.
    pub show_on_left_click: bool,
    /// Whether the icon is a macOS template icon (recolored by the system).
    pub icon_as_template: bool,
}

/// One entry of a tray menu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrayMenuItem {
    /// A clickable item; its `id` arrives as the `tray:{id}` event unless reserved.
    Item {
        /// Stable identifier.
        id: String,
        /// Display label.
        label: String,
        /// Whether the item is clickable.
        enabled: bool,
    },
    /// A separator line.
    Separator,
    /// A nested submenu.
    Submenu {
        /// Submenu label.
        label: String,
        /// Submenu entries.
        items: Vec<TrayMenuItem>,
    },
}

impl TrayMenuItem {
    /// A clickable item.
    pub fn item(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            label: label.into(),
            enabled: true,
        }
    }

    /// A disabled item.
    pub fn item_disabled(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            label: label.into(),
            enabled: false,
        }
    }

    /// A separator line.
    #[must_use]
    pub fn separator() -> Self {
        Self::Separator
    }

    /// A nested submenu.
    pub fn submenu(label: impl Into<String>, items: impl IntoIterator<Item = Self>) -> Self {
        Self::Submenu {
            label: label.into(),
            items: items.into_iter().collect(),
        }
    }
}

impl TrayConfig {
    /// An empty tray: icon only, no menu.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tooltip: None,
            menu_items: Vec::new(),
            show_on_left_click: true,
            icon_as_template: cfg!(target_os = "macos"),
        }
    }

    /// The standard tray: show / hide, separator, quit.
    pub fn standard(app_name: impl Into<String>) -> Self {
        let app_name = app_name.into();
        Self::new()
            .tooltip(&app_name)
            .menu_item("show", format!("Show {app_name}"))
            .menu_item("hide", format!("Hide {app_name}"))
            .separator()
            .menu_item("quit", "Quit")
    }

    /// The minimal tray: quit only.
    pub fn minimal(app_name: impl Into<String>) -> Self {
        Self::new().tooltip(app_name).menu_item("quit", "Quit")
    }

    /// Sets the hover tooltip.
    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Appends a clickable item.
    #[must_use]
    pub fn menu_item(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.menu_items.push(TrayMenuItem::item(id, label));
        self
    }

    /// Appends a disabled item.
    #[must_use]
    pub fn menu_item_disabled(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.menu_items.push(TrayMenuItem::item_disabled(id, label));
        self
    }

    /// Appends a separator.
    #[must_use]
    pub fn separator(mut self) -> Self {
        self.menu_items.push(TrayMenuItem::Separator);
        self
    }

    /// Appends a submenu.
    #[must_use]
    pub fn submenu(
        mut self,
        label: impl Into<String>,
        items: impl IntoIterator<Item = TrayMenuItem>,
    ) -> Self {
        self.menu_items.push(TrayMenuItem::submenu(label, items));
        self
    }

    /// Sets whether a left click shows the main window.
    #[must_use]
    pub fn show_on_left_click(mut self, show: bool) -> Self {
        self.show_on_left_click = show;
        self
    }

    /// Sets whether the icon is a macOS template icon.
    #[must_use]
    pub fn icon_as_template(mut self, template: bool) -> Self {
        self.icon_as_template = template;
        self
    }

    /// Builds the tray icon on the starting app.
    #[expect(
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub(crate) fn setup<R: Runtime>(&self, app: &App<R>) -> Result<(), NativeError> {
        let icon = app
            .default_window_icon()
            .cloned()
            .ok_or(NativeError::MissingTrayIcon)?;
        let menu = build_menu(app, &self.menu_items)?;
        let show_on_click = self.show_on_left_click;

        let mut builder = TrayIconBuilder::<R>::new()
            .icon(icon)
            .icon_as_template(self.icon_as_template)
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_tray_icon_event(move |tray, event| {
                if show_on_click
                    && let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    && let Some(window) = tray.app_handle().get_webview_window("main")
                {
                    if let Err(error) = window.show() {
                        warn!(error = ?error, "tray click could not show the main window");
                    }
                    if let Err(error) = window.set_focus() {
                        warn!(error = ?error, "tray click could not focus the main window");
                    }
                }
            })
            .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()));

        if let Some(tooltip) = &self.tooltip {
            builder = builder.tooltip(tooltip);
        }

        builder
            .build(app)
            .map_err(|source| NativeError::Tray { source })?;
        debug!(items = self.menu_items.len(), "system tray installed");
        Ok(())
    }
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self::new()
    }
}

fn build_menu<R: Runtime>(app: &App<R>, items: &[TrayMenuItem]) -> Result<Menu<R>, NativeError> {
    let mut builder = MenuBuilder::new(app);
    for item in items {
        match item {
            TrayMenuItem::Item { id, label, enabled } => {
                let entry = MenuItemBuilder::with_id(id, label)
                    .enabled(*enabled)
                    .build(app)
                    .map_err(|source| NativeError::Tray { source })?;
                builder = builder.item(&entry);
            }
            TrayMenuItem::Separator => builder = builder.separator(),
            TrayMenuItem::Submenu { label, items } => {
                let submenu = build_submenu(app, label, items)?;
                builder = builder.item(&submenu);
            }
        }
    }
    builder
        .build()
        .map_err(|source| NativeError::Tray { source })
}

fn build_submenu<R: Runtime>(
    app: &App<R>,
    label: &str,
    items: &[TrayMenuItem],
) -> Result<Submenu<R>, NativeError> {
    let mut builder = SubmenuBuilder::new(app, label);
    for item in items {
        match item {
            TrayMenuItem::Item { id, label, enabled } => {
                let entry = MenuItemBuilder::with_id(id, label)
                    .enabled(*enabled)
                    .build(app)
                    .map_err(|source| NativeError::Tray { source })?;
                builder = builder.item(&entry);
            }
            TrayMenuItem::Separator => builder = builder.separator(),
            TrayMenuItem::Submenu {
                label: nested_label,
                items: nested_items,
            } => {
                let nested = build_submenu(app, nested_label, nested_items)?;
                builder = builder.item(&nested);
            }
        }
    }
    builder
        .build()
        .map_err(|source| NativeError::Tray { source })
}

/// Routes a tray menu click: reserved ids act, everything else re-emits.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        "show" => {
            if let Some(window) = app.get_webview_window("main") {
                if let Err(error) = window.show() {
                    warn!(error = ?error, "tray show could not show the main window");
                }
                if let Err(error) = window.set_focus() {
                    warn!(error = ?error, "tray show could not focus the main window");
                }
            }
        }
        "hide" => {
            if let Some(window) = app.get_webview_window("main")
                && let Err(error) = window.hide()
            {
                warn!(error = ?error, "tray hide could not hide the main window");
            }
        }
        "quit" => app.exit(0),
        custom => {
            debug!(id = custom, "tray menu item re-emitted to the webview");
            if let Err(error) = app.emit(&format!("tray:{custom}"), ()) {
                warn!(error = ?error, id = custom, "tray event could not reach the webview");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TrayConfig, TrayMenuItem};

    #[test]
    fn standard_carries_show_hide_separator_quit_in_order() {
        let config = TrayConfig::standard("App");
        assert_eq!(config.tooltip.as_deref(), Some("App"));
        assert!(config.show_on_left_click);
        let ids: Vec<Option<&str>> = config
            .menu_items
            .iter()
            .map(|item| match item {
                TrayMenuItem::Item { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, vec![Some("show"), Some("hide"), None, Some("quit")]);
    }

    #[test]
    fn minimal_is_quit_only() {
        let config = TrayConfig::minimal("App");
        assert_eq!(config.menu_items.len(), 1);
        assert!(matches!(
            &config.menu_items[0],
            TrayMenuItem::Item { id, .. } if id == "quit"
        ));
    }

    #[test]
    fn submenus_nest() {
        let config = TrayConfig::new().submenu(
            "More",
            [
                TrayMenuItem::item("a", "A"),
                TrayMenuItem::submenu("Deeper", [TrayMenuItem::item("b", "B")]),
            ],
        );
        assert!(matches!(
            &config.menu_items[0],
            TrayMenuItem::Submenu { items, .. } if items.len() == 2
        ));
    }
}
