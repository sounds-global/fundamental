use std::sync::Arc;

use fundamental_server::Router;
use tauri::{Builder, Context, RunEvent, Wry};
use tracing::info;

use crate::invoke::BridgeState;
use crate::{
    ActionBridge, BridgeSet, ExitAction, InProcessBridge, NativeError, NativeEvent, NativeHandle,
    PlatformHooks, WindowConfig, invoke, page_protocol,
};

#[cfg(desktop)]
use crate::{MenuConfig, TrayConfig};

#[cfg(feature = "sync-host")]
use crate::sync_host::SyncHostState;
#[cfg(feature = "sync-host")]
use crate::{DeviceSliceRegistry, SyncHost, SyncHostConfig};

type BuilderFn = Box<dyn FnOnce(Builder<Wry>) -> Builder<Wry> + Send>;

/// The native app host: one builder for desktop and mobile.
///
/// Pages and actions go through the same swappable [`ActionBridge`](crate::ActionBridge).
/// `run` takes `tauri::generate_context!()` from the app crate.
///
/// # Example
///
/// ```no_run
/// # use fundamental_native::{NativeBuilder, WindowConfig};
/// # use fundamental_server::Router;
/// let router = Router::<()>::new(); // + register_pages / register_actions
/// let app = NativeBuilder::new()
///     .router(router, ())
///     .window(WindowConfig::main("My App", 1200.0, 800.0));
/// // In the app crate (where tauri.conf.json lives):
/// // app.run(tauri::generate_context!()).expect("native app failed");
/// ```
#[must_use = "a NativeBuilder does nothing until run"]
pub struct NativeBuilder {
    bridges: Option<BridgeSet>,
    windows: Vec<WindowConfig>,
    #[cfg(desktop)]
    tray: Option<TrayConfig>,
    #[cfg(desktop)]
    menu: Option<MenuConfig>,
    customizations: Vec<BuilderFn>,
    init_scripts: Vec<String>,
    hooks: Vec<Box<dyn PlatformHooks>>,
    #[cfg(feature = "deep-link")]
    deep_link: Option<crate::DeepLinkConfig>,
    #[cfg(feature = "push")]
    push: Option<Arc<dyn crate::PushRegistrar>>,
    #[cfg(feature = "sync-host")]
    sync_host: Option<(SyncHostConfig, DeviceSliceRegistry)>,
}

impl NativeBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self {
            bridges: None,
            windows: Vec::new(),
            #[cfg(desktop)]
            tray: None,
            #[cfg(desktop)]
            menu: None,
            customizations: Vec::new(),
            init_scripts: Vec::new(),
            hooks: Vec::new(),
            #[cfg(feature = "deep-link")]
            deep_link: None,
            #[cfg(feature = "push")]
            push: None,
            #[cfg(feature = "sync-host")]
            sync_host: None,
        }
    }

    /// Serves `router` over `state` in-process, dispatching anonymously.
    ///
    /// Shorthand for `.bridge(InProcessBridge::new(router, state))`.
    pub fn router<S>(self, router: Router<S>, state: S) -> Self
    where
        S: Clone + Send + Sync + 'static,
    {
        self.bridge(InProcessBridge::new(router, state))
    }

    /// Sets one bridge to serve pages and actions alike.
    pub fn bridge(self, bridge: impl ActionBridge) -> Self {
        self.bridges(BridgeSet::shared(bridge))
    }

    /// Sets a [`BridgeSet`], so pages and actions may be dispatched differently.
    ///
    /// `BridgeSet::serving_pages(InProcessBridge::new(router, state)).and_actions(remote)`
    /// renders on the device and dispatches to a server.
    pub fn bridges(mut self, bridges: BridgeSet) -> Self {
        self.bridges = Some(bridges);
        self
    }

    /// Adds a window (the first one added is the primary window).
    pub fn window(mut self, config: WindowConfig) -> Self {
        self.windows.push(config);
        self
    }

    /// Sets the system tray (desktop only).
    #[cfg(desktop)]
    pub fn tray(mut self, config: TrayConfig) -> Self {
        self.tray = Some(config);
        self
    }

    /// Sets the application menu (desktop only).
    #[cfg(desktop)]
    pub fn menu(mut self, config: MenuConfig) -> Self {
        self.menu = Some(config);
        self
    }

    /// Registers a Tauri plugin.
    pub fn plugin<P: tauri::plugin::Plugin<Wry> + 'static>(mut self, plugin: P) -> Self {
        self.customizations
            .push(Box::new(move |builder| builder.plugin(plugin)));
        self
    }

    /// Customizes the underlying Tauri builder directly, depending on Tauri's API.
    ///
    /// `.setup(..)` and `.invoke_handler(..)` are the framework's and always win; use
    /// [`PlatformHooks::on_setup`] instead.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use fundamental_native::NativeBuilder;
    /// struct Theme(&'static str);
    /// let app = NativeBuilder::new().customize(|builder| builder.manage(Theme("dark")));
    /// ```
    pub fn customize(
        mut self,
        customize: impl FnOnce(Builder<Wry>) -> Builder<Wry> + Send + 'static,
    ) -> Self {
        self.customizations.push(Box::new(customize));
        self
    }

    /// Adds a script evaluated in every window before any page script runs.
    pub fn initialization_script(mut self, script: impl Into<String>) -> Self {
        self.init_scripts.push(script.into());
        self
    }

    /// Adds platform hooks (the scaffold's per-OS `platforms/` module).
    pub fn hooks(mut self, hooks: impl PlatformHooks) -> Self {
        self.hooks.push(Box::new(hooks));
        self
    }

    /// Activates native deep-link and universal-link delivery (feature `deep-link`).
    ///
    /// Every link arrives at [`PlatformHooks::on_event`] as [`NativeEvent::Opened`] on every
    /// platform; the app declares its schemes in its platform config, see
    /// [`DeepLinkConfig`](crate::DeepLinkConfig).
    #[cfg(feature = "deep-link")]
    pub fn deep_link(mut self, config: crate::DeepLinkConfig) -> Self {
        self.deep_link = Some(config);
        self
    }

    /// Activates push notifications, recording device tokens through `registrar` (feature
    /// `push`).
    ///
    /// The framework owns the token's lifecycle; the app obtains the token and hands it to
    /// [`NativeHandle::push_registered`](crate::NativeHandle::push_registered). Pass `()` to
    /// record it elsewhere.
    #[cfg(feature = "push")]
    pub fn push_notifications(mut self, registrar: impl crate::PushRegistrar) -> Self {
        self.push = Some(Arc::new(registrar));
        self
    }

    /// Hosts the offline database in the app process: a [`SyncHost`] serving the webview's live
    /// reads over the reserved `/_fundamental/live/*` paths.
    #[cfg(feature = "sync-host")]
    pub fn sync_host(mut self, config: SyncHostConfig, slices: DeviceSliceRegistry) -> Self {
        self.sync_host = Some((config, slices));
        self
    }

    /// Builds and runs the app to completion.
    ///
    /// Blocks until the app exits.
    ///
    /// # Errors
    /// [`NativeError::MissingBridge`] without a router or bridge;
    /// [`NativeError::Run`] when the platform fails to build or run the app;
    /// window/tray/menu/hook failures abort startup with their own codes.
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub fn run(self, context: Context) -> Result<(), NativeError> {
        let bridges = self.bridges.ok_or(NativeError::MissingBridge)?;
        // Only the scheme handler serves pages.
        let bridge = bridges.actions();
        let windows = if self.windows.is_empty() {
            vec![WindowConfig::main("main", 1024.0, 768.0)]
        } else {
            self.windows
        };
        let init_scripts = self.init_scripts;
        let hooks: Arc<[Box<dyn PlatformHooks>]> = Arc::from(self.hooks);

        #[cfg(feature = "deep-link")]
        let deep_link = self.deep_link;
        // With the deep-link seam active, its plugin owns `Opened`, so a link fires once.
        let deep_link_active = {
            #[cfg(feature = "deep-link")]
            {
                deep_link.is_some()
            }
            #[cfg(not(feature = "deep-link"))]
            {
                false
            }
        };

        info!(
            windows = windows.len(),
            hooks = hooks.len(),
            customizations = self.customizations.len(),
            "native app starting"
        );

        // Customizations first, so the framework's single-slot hooks always win.
        let mut builder = Builder::default();
        // Single-instance must precede the app's plugins to intercept a second launch.
        #[cfg(feature = "deep-link")]
        if deep_link_active {
            builder = crate::deep_link::register_plugins(builder);
        }
        for customize in self.customizations {
            builder = customize(builder);
        }
        builder = builder
            .manage(BridgeState(Arc::clone(&bridge)))
            .invoke_handler(tauri::generate_handler![invoke::fundamental_invoke]);
        // Managed before setup: a launching notification tap arrives before setup runs.
        #[cfg(feature = "push")]
        if let Some(registrar) = self.push {
            builder = builder.manage(crate::push::PushState::new(registrar, Arc::clone(&hooks)));
        }
        builder = page_protocol::register(builder, bridges.pages());

        #[cfg(desktop)]
        let tray = self.tray;
        #[cfg(desktop)]
        let menu = self.menu;
        #[cfg(feature = "sync-host")]
        let sync_host = self.sync_host;
        let setup_bridge = Arc::clone(&bridge);
        let setup_hooks = Arc::clone(&hooks);

        builder = builder.setup(move |app| {
            for config in &windows {
                config.create(app, &init_scripts)?;
            }

            #[cfg(feature = "sync-host")]
            if let Some((config, slices)) = sync_host {
                use tauri::Manager;
                use tracing::warn;

                let data_dir = app.path().app_data_dir()?;
                std::fs::create_dir_all(&data_dir)?;
                let offline = config.offline_config(&data_dir);
                let emitter = NativeHandle::new(app.handle().clone(), Arc::clone(&setup_bridge));
                let host = SyncHost::start(offline, config, slices, move |table| {
                    let event = format!("fundamental:live:{table}");
                    // Best-effort: the webview re-reads on its next event.
                    if let Err(error) = emitter.emit(&event, ()) {
                        warn!(error = ?error, event, "live change event failed to emit");
                    }
                })?;
                app.manage(SyncHostState(host));
            }

            #[cfg(desktop)]
            {
                if let Some(tray) = &tray {
                    tray.setup(app)?;
                }
                if let Some(menu) = &menu {
                    menu.setup(app)?;
                }
            }

            let handle = NativeHandle::new(app.handle().clone(), Arc::clone(&setup_bridge));
            for hook in setup_hooks.iter() {
                hook.on_setup(&handle)
                    .map_err(|source| NativeError::Setup {
                        hook: hook.name().to_owned(),
                        source,
                    })?;
            }

            // A notification that arrived before the hooks existed is delivered now.
            #[cfg(feature = "push")]
            crate::push::release_pending(app, &handle, &setup_hooks);

            // After the hooks' setup, so the launching link reaches ready hooks.
            #[cfg(feature = "deep-link")]
            if let Some(config) = &deep_link {
                crate::deep_link::install_delivery(
                    app.handle(),
                    config,
                    Arc::clone(&setup_hooks),
                    Arc::clone(&setup_bridge),
                )?;
            }
            Ok(())
        });

        let app = builder
            .build(context)
            .map_err(|source| NativeError::Run { source })?;

        let run_bridge = Arc::clone(&bridge);
        app.run(move |app_handle, event| {
            if let RunEvent::ExitRequested { code, api, .. } = &event {
                let handle = NativeHandle::new(app_handle.clone(), Arc::clone(&run_bridge));
                let votes = hooks
                    .iter()
                    .map(|hook| (hook.name(), hook.on_exit_requested(&handle, *code)));
                if let Some(hook) = preventing_hook(*code, votes) {
                    info!(hook, "exit prevented by platform hook");
                    api.prevent_exit();
                }
            }
            if let Some(event) = NativeEvent::from_run_event(&event) {
                if !(deep_link_active && event.is_opened()) {
                    let handle = NativeHandle::new(app_handle.clone(), Arc::clone(&run_bridge));
                    for hook in hooks.iter() {
                        hook.on_event(&handle, &event);
                    }
                }
                // Launch and foreground are when a refused registration is retried.
                #[cfg(feature = "push")]
                if event.is_push_retry_moment() {
                    NativeHandle::new(app_handle.clone(), Arc::clone(&run_bridge))
                        .retry_push_registration();
                }
            }
        });
        Ok(())
    }
}

impl Default for NativeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the first hook that vetoed the exit, if the request is vetoable.
///
/// Programmatic exits (`code` is `Some`) are never vetoable; otherwise the first `Prevent` wins.
fn preventing_hook<'a>(
    code: Option<i32>,
    votes: impl IntoIterator<Item = (&'a str, ExitAction)>,
) -> Option<&'a str> {
    if code.is_some() {
        return None;
    }
    votes
        .into_iter()
        .find(|(_, vote)| *vote == ExitAction::Prevent)
        .map(|(hook, _)| hook)
}

impl std::fmt::Debug for NativeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("NativeBuilder");
        debug
            .field("has_bridge", &self.bridges.is_some())
            .field(
                "bridges_split",
                &self.bridges.as_ref().is_some_and(|b| !b.is_shared()),
            )
            .field("windows", &self.windows.len())
            .field("hooks", &self.hooks.len())
            .field("customizations", &self.customizations.len());
        #[cfg(desktop)]
        debug
            .field("has_tray", &self.tray.is_some())
            .field("has_menu", &self.menu.is_some());
        #[cfg(feature = "deep-link")]
        debug.field("has_deep_link", &self.deep_link.is_some());
        #[cfg(feature = "push")]
        debug.field("has_push", &self.push.is_some());
        #[cfg(feature = "sync-host")]
        debug.field("has_sync_host", &self.sync_host.is_some());
        debug.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeBuilder, preventing_hook};
    use crate::{ExitAction, NativeError, WindowConfig};
    use fundamental_core::Error as _;

    #[test]
    fn builder_accumulates_configuration() {
        let builder = NativeBuilder::new()
            .window(WindowConfig::main("App", 800.0, 600.0))
            .window(WindowConfig::secondary("settings", "Settings"))
            .initialization_script("console.log('hi')")
            .customize(|builder| builder)
            .hooks(());
        let debug = format!("{builder:?}");
        assert!(debug.contains("windows: 2"), "{debug}");
        assert!(debug.contains("hooks: 1"), "{debug}");
        assert!(debug.contains("customizations: 1"), "{debug}");
        assert!(debug.contains("has_bridge: false"), "{debug}");
    }

    #[cfg(feature = "deep-link")]
    #[test]
    fn deep_link_activation_is_recorded() {
        let builder = NativeBuilder::new().deep_link(crate::DeepLinkConfig::new(["openid4vp"]));
        assert!(
            format!("{builder:?}").contains("has_deep_link: true"),
            "the builder must record that deep-link delivery is active"
        );
    }

    #[cfg(feature = "push")]
    #[test]
    fn push_activation_is_recorded() {
        let builder = NativeBuilder::new().push_notifications(());
        assert!(
            format!("{builder:?}").contains("has_push: true"),
            "the builder must record that the push seam is active"
        );
    }

    #[cfg(feature = "push")]
    #[test]
    fn push_is_inactive_until_it_is_asked_for() {
        assert!(
            format!("{:?}", NativeBuilder::new()).contains("has_push: false"),
            "an app that never wired push must not carry the seam"
        );
    }

    #[test]
    fn first_preventing_hook_settles_a_user_exit() {
        let votes = [
            ("passive", ExitAction::Exit),
            ("tray", ExitAction::Prevent),
            ("later", ExitAction::Prevent),
        ];
        assert_eq!(preventing_hook(None, votes), Some("tray"));
    }

    #[test]
    fn unanimous_exit_votes_let_the_exit_proceed() {
        let votes = [("a", ExitAction::Exit), ("b", ExitAction::Exit)];
        assert_eq!(preventing_hook(None, votes), None);
    }

    #[test]
    fn programmatic_exits_are_not_vetoable() {
        let mut asked = false;
        let votes = std::iter::from_fn(|| {
            asked = true;
            Some(("tray", ExitAction::Prevent))
        })
        .take(1);
        assert_eq!(preventing_hook(Some(0), votes), None);
        assert!(!asked, "hooks must not be polled for a programmatic exit");
    }

    #[test]
    fn voting_short_circuits_after_the_first_prevent() {
        let mut polled = 0;
        let names = ["tray", "later"];
        let votes = names.iter().map(|name| {
            polled += 1;
            (*name, ExitAction::Prevent)
        });
        assert_eq!(preventing_hook(None, votes), Some("tray"));
        assert_eq!(polled, 1, "the second hook must not be asked");
    }

    #[test]
    fn missing_bridge_is_a_typed_error_before_any_platform_work() {
        // `run` needs a Context only an app crate can generate, so the guard's error is checked.
        let err = NativeError::MissingBridge;
        assert_eq!(err.error_code(), "NATIVE_008");
        assert!(format!("{err}").contains(".router"));
    }
}
