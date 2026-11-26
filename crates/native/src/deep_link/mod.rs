mod deep_link_config;

pub use deep_link_config::DeepLinkConfig;

use std::sync::Arc;

use tauri::{AppHandle, Builder, Wry};
use tracing::{info, warn};

use crate::{ActionBridge, NativeError, NativeEvent, NativeHandle, PlatformHooks};

/// Registers the deep-link plugins on the builder.
///
/// `single-instance` goes first, so a second launch forwards its link to the running instance.
pub(crate) fn register_plugins(builder: Builder<Wry>) -> Builder<Wry> {
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
        use tauri_plugin_deep_link::DeepLinkExt;
        // A second launch's argv may carry the deep link.
        app.deep_link().handle_cli_arguments(argv.iter());
    }));
    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
}

/// Installs inbound delivery: every opened URL, the launching one included, reaches the hooks
/// as [`NativeEvent::Opened`].
///
/// # Errors
/// Infallible in practice: desktop scheme registration only degrades.
pub(crate) fn install_delivery(
    app: &AppHandle,
    config: &DeepLinkConfig,
    hooks: Arc<[Box<dyn PlatformHooks>]>,
    bridge: Arc<dyn ActionBridge>,
) -> Result<(), NativeError> {
    register_schemes(app, config);
    deliver_live_links(app, &hooks, &bridge);
    deliver_launching_link(app, &hooks, &bridge);
    Ok(())
}

/// Registers the custom schemes at runtime where the desktop allows it.
fn register_schemes(app: &AppHandle, config: &DeepLinkConfig) {
    #[cfg(not(all(desktop, not(target_os = "macos"))))]
    let _ = app;
    // macOS registers schemes only from a bundled app's Info.plist.
    #[cfg(all(desktop, target_os = "macos"))]
    for scheme in config.schemes() {
        info!(
            scheme,
            "deep-link scheme declared; macOS routes it via the app bundle's Info.plist (a bare dev binary cannot receive it)"
        );
    }
    #[cfg(all(desktop, not(target_os = "macos")))]
    for scheme in config.schemes() {
        register_scheme(app, scheme);
    }
    #[cfg(not(desktop))]
    let _ = config;
}

/// Registers one custom scheme with the desktop; a failure only degrades.
#[cfg(all(desktop, not(target_os = "macos")))]
fn register_scheme(app: &AppHandle, scheme: &str) {
    use tauri_plugin_deep_link::DeepLinkExt;

    match app.deep_link().register(scheme) {
        Ok(()) => info!(scheme, "deep-link scheme registered"),
        Err(error) => not_registered(scheme, &error),
    }
}

#[cfg(all(desktop, not(target_os = "macos")))]
fn not_registered(scheme: &str, error: &impl std::fmt::Display) {
    warn!(scheme, error = %error, "deep-link scheme registration failed; the OS may not route it");
}

/// Dispatches every link opened while the app runs to the hooks.
fn deliver_live_links(
    app: &AppHandle,
    hooks: &Arc<[Box<dyn PlatformHooks>]>,
    bridge: &Arc<dyn ActionBridge>,
) {
    use tauri_plugin_deep_link::DeepLinkExt;
    let hooks_for_links = Arc::clone(hooks);
    let bridge_for_links = Arc::clone(bridge);
    let link_app = app.clone();
    app.deep_link().on_open_url(move |event| {
        let urls: Vec<String> = event
            .urls()
            .into_iter()
            .map(|url| url.to_string())
            .collect();
        dispatch_opened(&link_app, &hooks_for_links, &bridge_for_links, urls);
    });
}

/// Dispatches the link that launched the app, if one did.
fn deliver_launching_link(
    app: &AppHandle,
    hooks: &Arc<[Box<dyn PlatformHooks>]>,
    bridge: &Arc<dyn ActionBridge>,
) {
    use tauri_plugin_deep_link::DeepLinkExt;
    match app.deep_link().get_current() {
        Ok(Some(urls)) if !urls.is_empty() => {
            let urls: Vec<String> = urls.into_iter().map(|url| url.to_string()).collect();
            dispatch_opened(app, hooks, bridge, urls);
        }
        Ok(_) => {}
        Err(error) => warn!(error = %error, "reading the launching deep link failed"),
    }
}

/// Dispatches an [`NativeEvent::Opened`] to every hook.
///
/// The URLs are never logged, since a return link carries a single-use code.
fn dispatch_opened(
    app: &AppHandle,
    hooks: &Arc<[Box<dyn PlatformHooks>]>,
    bridge: &Arc<dyn ActionBridge>,
    urls: Vec<String>,
) {
    if urls.is_empty() {
        return;
    }
    let count = urls.len();
    let event = NativeEvent::Opened { urls };
    let handle = NativeHandle::new(app.clone(), Arc::clone(bridge));
    for hook in hooks.iter() {
        hook.on_event(&handle, &event);
    }
    info!(count, "deep link delivered to platform hooks");
}
