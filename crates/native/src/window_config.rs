use serde::{Deserialize, Serialize};
use tauri::{Runtime, WebviewUrl, webview::WebviewWindowBuilder};
use tracing::debug;

use crate::NativeError;
use crate::page_protocol::scheme_base;

/// One window of a native app: label, chrome, geometry, and the page it opens.
///
/// A window opens a router `path` over the in-process scheme; on mobile the geometry and chrome
/// are ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub struct WindowConfig {
    /// Unique window label (`"main"` for the primary window).
    pub label: String,
    /// Window title.
    pub title: String,
    /// Initial width in logical pixels.
    pub width: f64,
    /// Initial height in logical pixels.
    pub height: f64,
    /// Minimum size, if constrained.
    pub min_size: Option<(f64, f64)>,
    /// Maximum size, if constrained.
    pub max_size: Option<(f64, f64)>,
    /// Whether the window is resizable.
    pub resizable: bool,
    /// Whether to center the window on creation.
    pub center: bool,
    /// Whether to start fullscreen.
    pub fullscreen: bool,
    /// Whether to start maximized.
    pub maximized: bool,
    /// Whether the window is visible on creation.
    pub visible: bool,
    /// Whether the window has OS decorations (title bar, borders).
    pub decorations: bool,
    /// Whether the window stays above all others.
    pub always_on_top: bool,
    /// Whether the platform's native drag-drop handler is active; disable it for HTML5
    /// drag-and-drop.
    pub drag_drop_enabled: bool,
    /// The router path this window opens (default `/`).
    pub path: String,
    /// An absolute URL overriding `path` (for example a dev server).
    pub external_url: Option<String>,
}

impl WindowConfig {
    /// The primary window: label `main`, opening the router's `/` page.
    pub fn main(title: impl Into<String>, width: f64, height: f64) -> Self {
        Self {
            label: "main".into(),
            title: title.into(),
            width,
            height,
            min_size: None,
            max_size: None,
            resizable: true,
            center: true,
            fullscreen: false,
            maximized: false,
            visible: true,
            decorations: true,
            always_on_top: false,
            drag_drop_enabled: true,
            path: "/".into(),
            external_url: None,
        }
    }

    /// A secondary window with its own `label`, sized 800×600.
    pub fn secondary(label: impl Into<String>, title: impl Into<String>) -> Self {
        let mut config = Self::main(title, 800.0, 600.0);
        config.label = label.into();
        config.center = false;
        config
    }

    /// Sets the minimum size.
    #[must_use]
    pub fn min_size(mut self, width: f64, height: f64) -> Self {
        self.min_size = Some((width, height));
        self
    }

    /// Sets the maximum size.
    #[must_use]
    pub fn max_size(mut self, width: f64, height: f64) -> Self {
        self.max_size = Some((width, height));
        self
    }

    /// Sets whether the window is resizable.
    #[must_use]
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Sets whether the window centers on creation.
    #[must_use]
    pub fn center(mut self, center: bool) -> Self {
        self.center = center;
        self
    }

    /// Sets whether the window starts fullscreen.
    #[must_use]
    pub fn fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Sets whether the window starts maximized.
    #[must_use]
    pub fn maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    /// Sets whether the window is visible on creation.
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Sets whether the window has OS decorations.
    #[must_use]
    pub fn decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    /// Sets whether the window stays above all others.
    #[must_use]
    pub fn always_on_top(mut self, always_on_top: bool) -> Self {
        self.always_on_top = always_on_top;
        self
    }

    /// Sets whether the platform drag-drop handler is active.
    #[must_use]
    pub fn drag_drop_enabled(mut self, enabled: bool) -> Self {
        self.drag_drop_enabled = enabled;
        self
    }

    /// Sets the router path this window opens.
    #[must_use]
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Points the window at an absolute URL instead of the embedded router.
    #[must_use]
    pub fn external_url(mut self, url: impl Into<String>) -> Self {
        self.external_url = Some(url.into());
        self
    }

    /// The URL this window opens: `external_url`, or `path` on the in-process scheme.
    pub(crate) fn resolved_url(&self) -> String {
        match &self.external_url {
            Some(url) => url.clone(),
            None => format!("{}/{}", scheme_base(), self.path.trim_start_matches('/')),
        }
    }

    /// Creates this window on a running app, with `init_scripts` run before any page script.
    ///
    /// # Errors
    /// [`NativeError::Window`] when the URL is invalid or the platform refuses
    /// the window.
    pub(crate) fn create<R: Runtime, M: tauri::Manager<R>>(
        &self,
        manager: &M,
        init_scripts: &[String],
    ) -> Result<(), NativeError> {
        let url = self.resolved_url();
        let parsed: tauri::Url = url
            .parse()
            .map_err(|_| NativeError::InvalidUrl { url: url.clone() })?;

        let mut builder =
            WebviewWindowBuilder::new(manager, &self.label, WebviewUrl::External(parsed))
                .title(&self.title)
                .inner_size(self.width, self.height)
                .resizable(self.resizable)
                .fullscreen(self.fullscreen)
                .maximized(self.maximized)
                .visible(self.visible)
                .decorations(self.decorations)
                .always_on_top(self.always_on_top);

        if !self.drag_drop_enabled {
            builder = builder.disable_drag_drop_handler();
        }
        if let Some((width, height)) = self.min_size {
            builder = builder.min_inner_size(width, height);
        }
        if let Some((width, height)) = self.max_size {
            builder = builder.max_inner_size(width, height);
        }
        for script in init_scripts {
            builder = builder.initialization_script(script);
        }

        let window = builder.build().map_err(|source| NativeError::Window {
            label: self.label.clone(),
            source,
        })?;
        if self.center {
            window.center().map_err(|source| NativeError::Window {
                label: self.label.clone(),
                source,
            })?;
        }
        debug!(label = %self.label, %url, "window created");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::WindowConfig;

    #[test]
    fn main_defaults_to_the_root_page_on_the_scheme() {
        let config = WindowConfig::main("App", 1200.0, 800.0);
        assert_eq!(config.label, "main");
        assert!(config.resizable && config.center && config.visible);
        assert!(config.decorations && config.drag_drop_enabled);
        assert!(!config.fullscreen && !config.maximized && !config.always_on_top);
        let url = config.resolved_url();
        assert!(url.ends_with("/"), "{url}");
        assert!(url.contains("fundamental"), "{url}");
    }

    #[test]
    fn secondary_gets_its_own_label_and_skips_centering() {
        let config = WindowConfig::secondary("settings", "Settings");
        assert_eq!(config.label, "settings");
        assert!(!config.center);
    }

    #[test]
    fn path_joins_the_scheme_without_doubled_slashes() {
        let config = WindowConfig::main("App", 100.0, 100.0).path("/settings/profile");
        let url = config.resolved_url();
        assert!(url.ends_with("/settings/profile"), "{url}");
        assert!(!url.contains("//settings"), "{url}");
    }

    #[test]
    fn external_url_wins_over_path() {
        let config = WindowConfig::main("App", 100.0, 100.0)
            .path("/ignored")
            .external_url("http://localhost:1420/");
        assert_eq!(config.resolved_url(), "http://localhost:1420/");
    }
}
