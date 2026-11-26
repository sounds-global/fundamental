use serde::{Deserialize, Serialize};
use tauri::menu::{
    Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, Submenu, SubmenuBuilder,
};
use tauri::{App, Emitter, Runtime};
use tracing::{debug, warn};

use crate::NativeError;

/// The app's menu bar: a list of top-level submenus.
///
/// Custom item clicks re-emit to the webview as a `menu:{id}` event.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MenuConfig {
    /// The menu bar's top-level entries.
    pub items: Vec<MenuBarItem>,
}

/// One top-level menu-bar entry and its items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuBarItem {
    /// The entry's label (`"File"`, `"Edit"`, …).
    pub label: String,
    /// Its items, top to bottom.
    pub items: Vec<AppMenuItem>,
}

/// One item of an application menu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppMenuItem {
    /// A clickable item; its `id` arrives in the webview as a `menu:{id}` event.
    Item {
        /// Stable identifier.
        id: String,
        /// Display label.
        label: String,
        /// Keyboard accelerator (for example `"CmdOrCtrl+S"`).
        accelerator: Option<String>,
        /// Whether the item is clickable.
        enabled: bool,
    },
    /// A separator line.
    Separator,
    /// A predefined OS item (copy, paste, quit, …).
    Predefined(PredefinedItem),
    /// A nested submenu.
    Submenu {
        /// Submenu label.
        label: String,
        /// Submenu items.
        items: Vec<AppMenuItem>,
    },
}

/// The predefined, OS-provided menu items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PredefinedItem {
    /// Quit the application.
    Quit,
    /// Close the current window.
    CloseWindow,
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// Cut.
    Cut,
    /// Copy.
    Copy,
    /// Paste.
    Paste,
    /// Select all.
    SelectAll,
    /// Minimize the window.
    Minimize,
    /// Toggle fullscreen.
    Fullscreen,
    /// Hide the application (macOS).
    Hide,
    /// Hide other applications (macOS).
    HideOthers,
    /// Show all hidden applications (macOS).
    ShowAll,
    /// The About dialog.
    About,
    /// A separator (convenience variant).
    Separator,
    /// The Services menu (macOS).
    Services,
}

impl AppMenuItem {
    /// A clickable item with an optional accelerator.
    pub fn item(
        id: impl Into<String>,
        label: impl Into<String>,
        accelerator: Option<&str>,
    ) -> Self {
        Self::Item {
            id: id.into(),
            label: label.into(),
            accelerator: accelerator.map(str::to_owned),
            enabled: true,
        }
    }

    /// A disabled item.
    pub fn item_disabled(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            label: label.into(),
            accelerator: None,
            enabled: false,
        }
    }

    /// A separator line.
    #[must_use]
    pub fn separator() -> Self {
        Self::Separator
    }

    /// A predefined OS item.
    #[must_use]
    pub fn predefined(item: PredefinedItem) -> Self {
        Self::Predefined(item)
    }

    /// A nested submenu.
    pub fn submenu(label: impl Into<String>, items: impl IntoIterator<Item = Self>) -> Self {
        Self::Submenu {
            label: label.into(),
            items: items.into_iter().collect(),
        }
    }
}

impl MenuConfig {
    /// An empty menu bar.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The standard macOS menu bar: app, File, Edit, View, and Window menus.
    pub fn macos_standard(app_name: impl Into<String>) -> Self {
        let app_name = app_name.into();
        Self::new()
            .add_submenu(
                &app_name,
                vec![
                    AppMenuItem::Predefined(PredefinedItem::About),
                    AppMenuItem::Separator,
                    AppMenuItem::item("preferences", "Preferences...", Some("CmdOrCtrl+,")),
                    AppMenuItem::Separator,
                    AppMenuItem::Predefined(PredefinedItem::Services),
                    AppMenuItem::Separator,
                    AppMenuItem::Predefined(PredefinedItem::Hide),
                    AppMenuItem::Predefined(PredefinedItem::HideOthers),
                    AppMenuItem::Predefined(PredefinedItem::ShowAll),
                    AppMenuItem::Separator,
                    AppMenuItem::Predefined(PredefinedItem::Quit),
                ],
            )
            .add_submenu(
                "File",
                vec![
                    AppMenuItem::item("new", "New", Some("CmdOrCtrl+N")),
                    AppMenuItem::item("open", "Open...", Some("CmdOrCtrl+O")),
                    AppMenuItem::Separator,
                    AppMenuItem::Predefined(PredefinedItem::CloseWindow),
                ],
            )
            .add_submenu(
                "Edit",
                vec![
                    AppMenuItem::Predefined(PredefinedItem::Undo),
                    AppMenuItem::Predefined(PredefinedItem::Redo),
                    AppMenuItem::Separator,
                    AppMenuItem::Predefined(PredefinedItem::Cut),
                    AppMenuItem::Predefined(PredefinedItem::Copy),
                    AppMenuItem::Predefined(PredefinedItem::Paste),
                    AppMenuItem::Separator,
                    AppMenuItem::Predefined(PredefinedItem::SelectAll),
                ],
            )
            .add_submenu(
                "View",
                vec![AppMenuItem::Predefined(PredefinedItem::Fullscreen)],
            )
            .add_submenu(
                "Window",
                vec![AppMenuItem::Predefined(PredefinedItem::Minimize)],
            )
    }

    /// The minimal menu bar: just the Edit operations.
    #[must_use]
    pub fn minimal() -> Self {
        Self::new().add_submenu(
            "Edit",
            vec![
                AppMenuItem::Predefined(PredefinedItem::Undo),
                AppMenuItem::Predefined(PredefinedItem::Redo),
                AppMenuItem::Separator,
                AppMenuItem::Predefined(PredefinedItem::Cut),
                AppMenuItem::Predefined(PredefinedItem::Copy),
                AppMenuItem::Predefined(PredefinedItem::Paste),
                AppMenuItem::Separator,
                AppMenuItem::Predefined(PredefinedItem::SelectAll),
            ],
        )
    }

    /// Appends a top-level submenu.
    #[must_use]
    pub fn add_submenu(
        mut self,
        label: impl Into<String>,
        items: impl IntoIterator<Item = AppMenuItem>,
    ) -> Self {
        self.items.push(MenuBarItem {
            label: label.into(),
            items: items.into_iter().collect(),
        });
        self
    }

    /// Installs the menu bar on the starting app.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn setup<R: Runtime>(&self, app: &App<R>) -> Result<(), NativeError> {
        let menu = self.build_menu(app)?;
        app.set_menu(menu)
            .map_err(|source| NativeError::Menu { source })?;
        app.on_menu_event(|app, event| {
            let id = event.id.as_ref();
            debug!(id, "menu item re-emitted to the webview");
            if let Err(error) = app.emit(&format!("menu:{id}"), ()) {
                warn!(error = ?error, id, "menu event could not reach the webview");
            }
        });
        debug!(submenus = self.items.len(), "application menu installed");
        Ok(())
    }

    fn build_menu<R: Runtime>(&self, app: &App<R>) -> Result<Menu<R>, NativeError> {
        let mut builder = MenuBuilder::new(app);
        for entry in &self.items {
            let submenu = build_submenu(app, &entry.label, &entry.items)?;
            builder = builder.item(&submenu);
        }
        builder
            .build()
            .map_err(|source| NativeError::Menu { source })
    }
}

fn build_submenu<R: Runtime>(
    app: &App<R>,
    label: &str,
    items: &[AppMenuItem],
) -> Result<Submenu<R>, NativeError> {
    let mut builder = SubmenuBuilder::new(app, label);
    for item in items {
        match item {
            AppMenuItem::Item {
                id,
                label,
                accelerator,
                enabled,
            } => {
                let mut item_builder = MenuItemBuilder::with_id(id, label).enabled(*enabled);
                if let Some(accelerator) = accelerator {
                    item_builder = item_builder.accelerator(accelerator);
                }
                let entry = item_builder
                    .build(app)
                    .map_err(|source| NativeError::Menu { source })?;
                builder = builder.item(&entry);
            }
            AppMenuItem::Separator => builder = builder.separator(),
            AppMenuItem::Predefined(predefined) => {
                let entry = to_tauri(app, *predefined)?;
                builder = builder.item(&entry);
            }
            AppMenuItem::Submenu {
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
        .map_err(|source| NativeError::Menu { source })
}

fn to_tauri<R: Runtime>(
    app: &App<R>,
    item: PredefinedItem,
) -> Result<PredefinedMenuItem<R>, NativeError> {
    let built = match item {
        PredefinedItem::Quit => PredefinedMenuItem::quit(app, Some("Quit")),
        PredefinedItem::CloseWindow => PredefinedMenuItem::close_window(app, Some("Close Window")),
        PredefinedItem::Undo => PredefinedMenuItem::undo(app, Some("Undo")),
        PredefinedItem::Redo => PredefinedMenuItem::redo(app, Some("Redo")),
        PredefinedItem::Cut => PredefinedMenuItem::cut(app, Some("Cut")),
        PredefinedItem::Copy => PredefinedMenuItem::copy(app, Some("Copy")),
        PredefinedItem::Paste => PredefinedMenuItem::paste(app, Some("Paste")),
        PredefinedItem::SelectAll => PredefinedMenuItem::select_all(app, Some("Select All")),
        PredefinedItem::Minimize => PredefinedMenuItem::minimize(app, Some("Minimize")),
        PredefinedItem::Fullscreen => PredefinedMenuItem::fullscreen(app, Some("Fullscreen")),
        PredefinedItem::Hide => PredefinedMenuItem::hide(app, Some("Hide")),
        PredefinedItem::HideOthers => PredefinedMenuItem::hide_others(app, Some("Hide Others")),
        PredefinedItem::ShowAll => PredefinedMenuItem::show_all(app, Some("Show All")),
        PredefinedItem::About => PredefinedMenuItem::about(app, Some("About"), None),
        PredefinedItem::Separator => PredefinedMenuItem::separator(app),
        PredefinedItem::Services => PredefinedMenuItem::services(app, Some("Services")),
    };
    built.map_err(|source| NativeError::Menu { source })
}

#[cfg(test)]
mod tests {
    use super::{AppMenuItem, MenuConfig, PredefinedItem};

    #[test]
    fn macos_standard_carries_the_five_menus_in_order() {
        let config = MenuConfig::macos_standard("App");
        let labels: Vec<&str> = config.items.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, vec!["App", "File", "Edit", "View", "Window"]);
        assert!(matches!(
            config.items[0].items.last(),
            Some(AppMenuItem::Predefined(PredefinedItem::Quit))
        ));
    }

    #[test]
    fn minimal_is_edit_only() {
        let config = MenuConfig::minimal();
        assert_eq!(config.items.len(), 1);
        assert_eq!(config.items[0].label, "Edit");
    }

    #[test]
    fn add_submenu_appends_in_order() {
        let config = MenuConfig::new()
            .add_submenu("One", vec![])
            .add_submenu("Two", vec![AppMenuItem::item("x", "X", None)]);
        assert_eq!(config.items.len(), 2);
        assert_eq!(config.items[1].label, "Two");
    }
}
