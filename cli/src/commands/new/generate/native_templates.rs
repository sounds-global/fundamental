//! File-content templates for the native (Tauri) surface's crate tree, `apps/<app>/native/`.

use super::ProjectPlan;
use super::templates::TRACING_INIT;

/// The neutral starter icon set from `cargo tauri icon`, so `fun build --bundle` packages as is.
pub(super) const ICON_FILES: &[(&str, &[u8])] = &[
    ("icon.png", include_bytes!("icons/icon.png")),
    ("icon.icns", include_bytes!("icons/icon.icns")),
    ("icon.ico", include_bytes!("icons/icon.ico")),
    ("32x32.png", include_bytes!("icons/32x32.png")),
    ("128x128.png", include_bytes!("icons/128x128.png")),
    ("128x128@2x.png", include_bytes!("icons/128x128@2x.png")),
];

/// Matches `templates::FUNDAMENTAL_GIT`.
const FUNDAMENTAL_GIT: &str = "https://github.com/sounds-global/fundamental";

/// The native crate's package name for `plan`.
pub(super) fn native_crate_name(plan: &ProjectPlan) -> String {
    format!("{}_native", plan.app_crate_name)
}

/// The hooks plugin's package name, hyphenated: `tauri-plugin`'s build script rejects underscores.
pub(super) fn hooks_crate_name(plan: &ProjectPlan) -> String {
    format!("{}-native-hooks", plan.app_crate_name.replace('_', "-"))
}

/// The hooks plugin crate's library (import) name for `plan`.
pub(super) fn hooks_lib_name(plan: &ProjectPlan) -> String {
    format!("{}_native_hooks", plan.app_crate_name)
}

/// The reverse-domain bundle identifier (Apple forbids underscores).
fn bundle_identifier(plan: &ProjectPlan) -> String {
    format!(
        "com.{}.{}",
        plan.name.replace('_', "-"),
        plan.app_name.replace('_', "-")
    )
}

/// The Android/Kotlin package for the hooks plugin (Java forbids hyphens).
fn android_package(plan: &ProjectPlan) -> String {
    format!(
        "com.{}.{}.hooks",
        plan.name.replace('-', "_"),
        plan.app_crate_name
    )
}

/// The native crate's `Cargo.toml`: a mobile-capable library beside a desktop binary.
pub(super) fn native_cargo_toml(plan: &ProjectPlan) -> String {
    let crate_name = native_crate_name(plan);
    let hooks = hooks_crate_name(plan);
    let hooks_lib = hooks_lib_name(plan);
    let features = if plan.sync {
        "[\"native\", \"native-sync-host\"]"
    } else {
        "[\"native\"]"
    };
    format!(
        "[package]\n\
         name = \"{crate_name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\n\
         [lints]\n\
         workspace = true\n\n\
         # Mobile builds link the library (static on iOS); desktop runs the binary.\n\
         [lib]\n\
         crate-type = [\"staticlib\", \"cdylib\", \"rlib\"]\n\n\
         [[bin]]\n\
         name = \"{crate_name}\"\n\
         path = \"src/main.rs\"\n\n\
         [dependencies]\n\
         fundamental = {{ workspace = true, features = {features} }}\n\
         app = {{ path = \"..\", package = \"{package}\" }}\n\
         # Imported as `{hooks_lib}` (the hyphenated package's lib name).\n\
         {hooks} = {{ path = \"hooks\" }}\n\
         leptos = {{ version = \"0.8\", default-features = false, features = [\"islands\"] }}\n\
         tauri = {{ workspace = true }}\n\
         tracing = {{ workspace = true }}\n\
         tracing-subscriber = {{ workspace = true }}\n\n\
         [build-dependencies]\n\
         tauri-build = {{ workspace = true }}\n\
         fundamental_build = {{ git = \"{git}\" }}\n",
        package = plan.app_crate_name,
        git = FUNDAMENTAL_GIT,
    )
}

/// The native crate's `build.rs`: the Tauri context plus the app's `../pages`.
pub(super) fn native_build_rs() -> String {
    "//! Compiles the Tauri context, and the app crate's `../pages` for in-process rendering.\n\n\
     fn main() -> std::process::ExitCode {\n\
     \x20   tauri_build::build();\n\
     \x20   fundamental_build::PagesBuild::run_build_script_in(\"..\", fundamental_build::Hydration::Plain)\n\
     }\n"
        .to_string()
}

/// The native crate's `src/lib.rs`: the shared entry for desktop and mobile.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(super) fn native_lib_rs(plan: &ProjectPlan) -> String {
    let app = &plan.app_name;
    let hooks = hooks_lib_name(plan);
    // On a sync app the host owns the durable offline database the webview reads.
    let sync_host_items = if plan.sync {
        format!(
            "\n// List the same `domain::sync::*` markers the server mounts with `mount_slices!`.\n\
             fundamental::mount_device_slices! {{}}\n\n\
             /// The dev sync server (`fun dev {app}`), or `FUNDAMENTAL_SYNC_URL` and `FUNDAMENTAL_SYNC_TOKEN`.\n\
             fn sync_host_config() -> fundamental::native::SyncHostConfig {{\n\
             \x20   use fundamental::core::secrets::SecretString;\n\
             \x20   use fundamental::native::SyncHostConfig;\n\n\
             \x20   let url = std::env::var(\"FUNDAMENTAL_SYNC_URL\")\n\
             \x20       .unwrap_or_else(|_| \"ws://localhost:{sync_port}\".to_string());\n\
             \x20   let mut config = SyncHostConfig::new(url);\n\
             \x20   if let Ok(token) = std::env::var(\"FUNDAMENTAL_SYNC_TOKEN\") {{\n\
             \x20       config = config.with_token(SecretString::new(token));\n\
             \x20   }}\n\
             \x20   config\n\
             }}\n",
            sync_port = plan.sync_port,
        )
    } else {
        String::new()
    };
    let sync_host_line = if plan.sync {
        "\x20       .sync_host(sync_host_config(), device_slices())\n"
    } else {
        ""
    };
    format!(
        "//! `{app}` app: native (desktop + mobile) surface, its pages rendered in-process.\n\n\
         include!(concat!(env!(\"OUT_DIR\"), \"/pages.rs\"));\n\n\
         mod platform_hooks;\n\n\
         use app::register_actions;\n\
         #[expect(unused_imports, reason = \"no page calls an action yet; remove this line when one does\")]\n\
         use app::actions;\n\n\
         /// Builds the router and runs the native app to completion.\n\
         #[cfg_attr(mobile, tauri::mobile_entry_point)]\n\
         pub fn run() {{\n\
         \x20   use fundamental::native::{{NativeBuilder, WindowConfig}};\n\
         \x20   use tracing::error;\n\
         \x20   use tracing_subscriber::EnvFilter;\n\n\
         {TRACING_INIT}\n\
         \x20   let router = match router() {{\n\
         \x20       Ok(router) => router,\n\
         \x20       Err(error) => {{\n\
         \x20           error!(error = ?error, \"router setup failed\");\n\
         \x20           std::process::exit(1);\n\
         \x20       }}\n\
         \x20   }};\n\n\
         \x20   let outcome = NativeBuilder::new()\n\
         \x20       .router(router, ())\n\
         \x20       .window(WindowConfig::main(\"{app}\", 1200.0, 800.0))\n\
         \x20       .hooks(platform_hooks::hooks())\n\
         \x20       .plugin({hooks}::init())\n\
         \x20       // `.customize(|builder| ..)` reaches the raw Tauri builder.\n\
         {sync_host_line}\
         \x20       .run(tauri::generate_context!());\n\
         \x20   if let Err(error) = outcome {{\n\
         \x20       error!(error = ?error, \"native app failed\");\n\
         \x20       std::process::exit(1);\n\
         \x20   }}\n\
         }}\n\
         {sync_host_items}\n\
         /// The app's router: every page, every mounted action, and the webview bundle.\n\
         fn router() -> Result<fundamental::web::Router<()>, fundamental::AnyError> {{\n\
         \x20   let dist = concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/dist\");\n\
         \x20   Ok(\n\
         \x20       register_actions(register_pages(fundamental::web::Router::<()>::new())?)?\n\
         \x20           .static_dir(\"/_fundamental\", dist)?,\n\
         \x20   )\n\
         }}\n"
    )
}

/// The native crate's `src/main.rs`: the desktop shim over the lib's `run()`.
pub(super) fn native_main_rs(plan: &ProjectPlan) -> String {
    format!(
        "//! `{app}` native app: desktop entry (mobile enters through `lib.rs`).\n\n\
         fn main() {{\n\
         \x20   {crate_name}::run();\n\
         }}\n",
        app = plan.app_name,
        crate_name = native_crate_name(plan),
    )
}

/// The generated `src/platform_hooks.rs`: the one compile-time switch into `platforms/`.
pub(super) fn native_platform_hooks_rs() -> String {
    "//! Selects this build's platform hooks from `../platforms/`, so hook files need no `#[cfg]`.\n\n\
     #[cfg(target_os = \"macos\")]\n\
     #[path = \"../platforms/macos/hooks.rs\"]\n\
     mod platform;\n\n\
     #[cfg(target_os = \"windows\")]\n\
     #[path = \"../platforms/windows/hooks.rs\"]\n\
     mod platform;\n\n\
     // Linux and the other desktop unixes share the Linux hooks.\n\
     #[cfg(not(any(\n\
     \x20   target_os = \"macos\",\n\
     \x20   target_os = \"windows\",\n\
     \x20   target_os = \"ios\",\n\
     \x20   target_os = \"android\"\n\
     )))]\n\
     #[path = \"../platforms/linux/hooks.rs\"]\n\
     mod platform;\n\n\
     // On mobile Swift/Kotlin code joins through the hooks plugin.\n\
     #[cfg(any(target_os = \"ios\", target_os = \"android\"))]\n\
     mod platform {\n\
     \x20   pub(crate) fn hooks() -> impl fundamental::native::PlatformHooks {}\n\
     }\n\n\
     pub(crate) use platform::hooks;\n"
        .to_string()
}

/// One desktop OS's starter `platforms/<os>/hooks.rs`.
pub(super) fn native_desktop_hooks_rs(os: &str) -> String {
    format!(
        "//! {os} platform hooks: hand-written {os}-only code joins the app here.\n\n\
         use fundamental::AnyError;\n\
         use fundamental::native::{{NativeEvent, NativeHandle, PlatformHooks}};\n\
         use tracing::debug;\n\n\
         /// This platform's hooks.\n\
         pub(crate) struct Hooks;\n\n\
         impl PlatformHooks for Hooks {{\n\
         \x20   fn name(&self) -> &'static str {{\n\
         \x20       \"{os}\"\n\
         \x20   }}\n\n\
         \x20   fn on_setup(&self, _app: &NativeHandle) -> Result<(), AnyError> {{\n\
         \x20       // Runs once at startup, after windows exist.\n\
         \x20       Ok(())\n\
         \x20   }}\n\n\
         \x20   // Window focus, menu and tray selections and exit; `on_exit_requested` can veto an exit.\n\
         \x20   fn on_event(&self, _app: &NativeHandle, event: &NativeEvent) {{\n\
         \x20       if let NativeEvent::MenuSelected {{ id }} = event {{\n\
         \x20           debug!(id, \"menu item selected\");\n\
         \x20       }}\n\
         \x20   }}\n\
         }}\n\n\
         /// The instance `src/platform_hooks.rs` hands to the builder.\n\
         pub(crate) fn hooks() -> impl PlatformHooks {{\n\
         \x20   Hooks\n\
         }}\n"
    )
}

/// The `platforms/README.md` explaining where each kind of native code goes.
pub(super) fn native_platforms_readme(plan: &ProjectPlan) -> String {
    let hooks = hooks_crate_name(plan);
    format!(
        "# Platform code\n\n\
         Hand-written native code lives here, in stable committed folders — never in\n\
         `../gen/`, which Tauri regenerates.\n\n\
         - `macos/`, `windows/`, `linux/` — Rust hook modules (`hooks.rs`), selected\n\
         \x20 per target by `../src/platform_hooks.rs`. They receive the app's\n\
         \x20 `NativeHandle`: the action bridge, events to the webview, window control.\n\
         - `ios/` — a Swift package (`Sources/HooksPlugin.swift`) compiled into the\n\
         \x20 app by the `{hooks}` plugin on every iOS build.\n\
         - `android/` — a Kotlin library module (`src/main/java/HooksPlugin.kt`),\n\
         \x20 compiled in the same way on every Android build.\n\n\
         Rust reaches the mobile sides through `{hooks}` (`run_mobile_plugin`); on\n\
         desktop the same calls answer in Rust. Declarative per-platform config\n\
         (Info.plist entries, gradle settings, signing) lives in `../gen/` after\n\
         `fun dev {app} native --platform ios|android` first materializes it — commit\n\
         `gen/`, edit it there, and keep code here.\n",
        app = plan.app_name,
    )
}

/// The locked-down CSP: `'self'` and the in-process scheme, plus Tauri IPC and the sync WebSocket.
pub(super) const NATIVE_CSP: &str = "default-src 'self' fundamental: http://fundamental.localhost; \
     script-src 'self' fundamental: http://fundamental.localhost 'wasm-unsafe-eval'; \
     style-src 'self' fundamental: http://fundamental.localhost 'unsafe-inline'; \
     img-src 'self' fundamental: http://fundamental.localhost data:; \
     connect-src 'self' fundamental: http://fundamental.localhost ipc: http://ipc.localhost ws://localhost:* wss:";

/// The native crate's `tauri.conf.json`.
pub(super) fn native_tauri_conf_json(plan: &ProjectPlan) -> String {
    format!(
        "{{\n\
         \x20 \"$schema\": \"https://schema.tauri.app/config/2\",\n\
         \x20 \"productName\": \"{app}\",\n\
         \x20 \"version\": \"0.1.0\",\n\
         \x20 \"identifier\": \"{identifier}\",\n\
         \x20 \"build\": {{}},\n\
         \x20 \"app\": {{\n\
         \x20   \"windows\": [],\n\
         \x20   \"security\": {{\n\
         \x20     \"csp\": \"{csp}\"\n\
         \x20   }}\n\
         \x20 }},\n\
         \x20 \"bundle\": {{\n\
         \x20   \"active\": true,\n\
         \x20   \"icon\": [\"icons/32x32.png\", \"icons/128x128.png\", \"icons/128x128@2x.png\", \"icons/icon.icns\", \"icons/icon.ico\"],\n\
         \x20   \"iOS\": {{\n\
         \x20     \"minimumSystemVersion\": \"13.0\"\n\
         \x20   }},\n\
         \x20   \"android\": {{\n\
         \x20     \"minSdkVersion\": 24\n\
         \x20   }}\n\
         \x20 }}\n\
         }}\n",
        app = plan.app_name,
        identifier = bundle_identifier(plan),
        csp = NATIVE_CSP,
    )
}

/// The default capability: the main window with the core permission set.
pub(super) fn native_capabilities_json() -> String {
    "{\n\
     \x20 \"$schema\": \"../gen/schemas/desktop-schema.json\",\n\
     \x20 \"identifier\": \"default\",\n\
     \x20 \"description\": \"Default capability for the main window.\",\n\
     \x20 \"windows\": [\"main\"],\n\
     \x20 \"permissions\": [\"core:default\"]\n\
     }\n"
    .to_string()
}

/// The hooks plugin crate's `Cargo.toml`.
pub(super) fn hooks_cargo_toml(plan: &ProjectPlan) -> String {
    format!(
        "[package]\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         # Required by tauri-plugin's build script (must equal the package name).\n\
         links = \"{name}\"\n\n\
         [lints]\n\
         workspace = true\n\n\
         [dependencies]\n\
         serde = {{ workspace = true }}\n\
         tauri = {{ workspace = true }}\n\
         tracing = {{ workspace = true }}\n\n\
         [build-dependencies]\n\
         tauri-plugin = {{ workspace = true, features = [\"build\"] }}\n",
        name = hooks_crate_name(plan),
    )
}

/// The hooks plugin's `build.rs`: point the mobile builds at `../platforms`.
pub(super) fn hooks_build_rs() -> String {
    "//! Compiles `../platforms/ios` and `../platforms/android` into every mobile build.\n\n\
     const COMMANDS: &[&str] = &[];\n\n\
     fn main() {\n\
     \x20   tauri_plugin::Builder::new(COMMANDS)\n\
     \x20       .android_path(\"../platforms/android\")\n\
     \x20       .ios_path(\"../platforms/ios\")\n\
     \x20       .build();\n\
     }\n"
    .to_string()
}

/// The hooks plugin's `src/lib.rs`: the Rust seam over the mobile plugin classes.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(super) fn hooks_lib_rs(plan: &ProjectPlan) -> String {
    let app = &plan.app_name;
    let name = hooks_lib_name(plan);
    let plugin = hooks_crate_name(plan);
    let package = android_package(plan);
    format!(
        "//! `{app}`'s platform-hooks plugin: `use {name}::HooksExt; handle.hooks().ping(..)?;`\n\n\
         use serde::{{Deserialize, Serialize}};\n\
         use tauri::plugin::{{Builder, TauriPlugin}};\n\
         use tauri::{{Manager, Runtime}};\n\n\
         #[cfg(any(target_os = \"android\", target_os = \"ios\"))]\n\
         use tauri::plugin::PluginHandle;\n\n\
         #[cfg(target_os = \"ios\")]\n\
         tauri::ios_plugin_binding!(init_plugin_hooks);\n\n\
         /// The example round-trip payload; replace with your real calls.\n\
         #[derive(Debug, Clone, Serialize, Deserialize)]\n\
         pub struct Ping {{\n\
         \x20   /// Echoed value.\n\
         \x20   pub value: String,\n\
         }}\n\n\
         /// Access to the platform hooks from Rust.\n\
         #[cfg(any(target_os = \"android\", target_os = \"ios\"))]\n\
         pub struct Hooks<R: Runtime>(PluginHandle<R>);\n\
         /// Access to the platform hooks from Rust (desktop: answered in Rust).\n\
         // `fn() -> R` keeps the marker `Send + Sync` whatever `R` is.\n\
         #[cfg(not(any(target_os = \"android\", target_os = \"ios\")))]\n\
         pub struct Hooks<R: Runtime>(std::marker::PhantomData<fn() -> R>);\n\n\
         impl<R: Runtime> Hooks<R> {{\n\
         \x20   /// Round-trips `ping` through the platform side.\n\
         \x20   pub fn ping(&self, payload: Ping) -> Result<Ping, tauri::Error> {{\n\
         \x20       #[cfg(any(target_os = \"android\", target_os = \"ios\"))]\n\
         \x20       {{\n\
         \x20           self.0.run_mobile_plugin(\"ping\", payload).map_err(Into::into)\n\
         \x20       }}\n\
         \x20       #[cfg(not(any(target_os = \"android\", target_os = \"ios\")))]\n\
         \x20       {{\n\
         \x20           Ok(payload)\n\
         \x20       }}\n\
         \x20   }}\n\
         }}\n\n\
         /// Reaches the hooks from any Tauri manager (app handle, window).\n\
         pub trait HooksExt<R: Runtime> {{\n\
         \x20   /// The managed [`Hooks`] instance.\n\
         \x20   fn hooks(&self) -> &Hooks<R>;\n\
         }}\n\n\
         impl<R: Runtime, T: Manager<R>> HooksExt<R> for T {{\n\
         \x20   fn hooks(&self) -> &Hooks<R> {{\n\
         \x20       self.state::<Hooks<R>>().inner()\n\
         \x20   }}\n\
         }}\n\n\
         /// Initializes the plugin; handed to the native builder in `../src/lib.rs`.\n\
         pub fn init<R: Runtime>() -> TauriPlugin<R> {{\n\
         \x20   Builder::new(\"{plugin}\")\n\
         \x20       .setup(|app, api| {{\n\
         \x20           #[cfg(target_os = \"android\")]\n\
         \x20           let hooks = Hooks(api.register_android_plugin(\"{package}\", \"HooksPlugin\")?);\n\
         \x20           #[cfg(target_os = \"ios\")]\n\
         \x20           let hooks = Hooks(api.register_ios_plugin(init_plugin_hooks)?);\n\
         \x20           #[cfg(not(any(target_os = \"android\", target_os = \"ios\")))]\n\
         \x20           let hooks = {{\n\
         \x20               let _ = &api;\n\
         \x20               Hooks::<R>(std::marker::PhantomData)\n\
         \x20           }};\n\
         \x20           app.manage(hooks);\n\
         \x20           Ok(())\n\
         \x20       }})\n\
         \x20       .build()\n\
         }}\n"
    )
}

/// `platforms/ios/Package.swift`, named for the hooks crate so two apps never collide.
pub(super) fn ios_package_swift(plan: &ProjectPlan) -> String {
    let name = hooks_crate_name(plan);
    format!(
        "// swift-tools-version:5.3\n\
         import PackageDescription\n\n\
         let package = Package(\n\
         \x20   name: \"{name}\",\n\
         \x20   platforms: [\n\
         \x20       .macOS(.v10_13),\n\
         \x20       .iOS(.v13),\n\
         \x20   ],\n\
         \x20   products: [\n\
         \x20       .library(\n\
         \x20           name: \"{name}\",\n\
         \x20           type: .static,\n\
         \x20           targets: [\"{name}\"]),\n\
         \x20   ],\n\
         \x20   dependencies: [\n\
         \x20       // Materialized by the Tauri CLI on the first iOS build.\n\
         \x20       .package(name: \"Tauri\", path: \"../.tauri/tauri-api\")\n\
         \x20   ],\n\
         \x20   targets: [\n\
         \x20       .target(\n\
         \x20           name: \"{name}\",\n\
         \x20           dependencies: [\n\
         \x20               .byName(name: \"Tauri\")\n\
         \x20           ],\n\
         \x20           path: \"Sources\")\n\
         \x20   ]\n\
         )\n"
    )
}

/// `platforms/ios/Sources/HooksPlugin.swift`: the hand-written Swift seam.
pub(super) fn ios_hooks_plugin_swift() -> String {
    "import SwiftRs\n\
     import Tauri\n\
     import UIKit\n\
     import WebKit\n\n\
     class PingArgs: Decodable {\n\
     \x20   let value: String?\n\
     }\n\n\
     // Called from Rust through the hooks plugin's `run_mobile_plugin`.\n\
     class HooksPlugin: Plugin {\n\
     \x20   @objc public func ping(_ invoke: Invoke) throws {\n\
     \x20       let args = try invoke.parseArgs(PingArgs.self)\n\
     \x20       invoke.resolve([\"value\": args.value ?? \"\"])\n\
     \x20   }\n\
     }\n\n\
     @_cdecl(\"init_plugin_hooks\")\n\
     func initPlugin() -> Plugin {\n\
     \x20   return HooksPlugin()\n\
     }\n"
    .to_string()
}

/// `platforms/android/build.gradle.kts`, following Tauri's official template.
pub(super) fn android_build_gradle(plan: &ProjectPlan) -> String {
    format!(
        "plugins {{\n\
         \x20   id(\"com.android.library\")\n\
         \x20   id(\"org.jetbrains.kotlin.android\")\n\
         }}\n\n\
         android {{\n\
         \x20   namespace = \"{package}\"\n\
         \x20   compileSdk = 36\n\n\
         \x20   defaultConfig {{\n\
         \x20       minSdk = 24\n\
         \x20   }}\n\n\
         \x20   compileOptions {{\n\
         \x20       sourceCompatibility = JavaVersion.VERSION_1_8\n\
         \x20       targetCompatibility = JavaVersion.VERSION_1_8\n\
         \x20   }}\n\
         \x20   kotlinOptions {{\n\
         \x20       jvmTarget = \"1.8\"\n\
         \x20   }}\n\
         }}\n\n\
         dependencies {{\n\
         \x20   implementation(\"androidx.core:core-ktx:1.9.0\")\n\
         \x20   // Wired into the app's gradle project by the Tauri CLI.\n\
         \x20   implementation(project(\":tauri-android\"))\n\
         }}\n",
        package = android_package(plan),
    )
}

/// `platforms/android/src/main/AndroidManifest.xml`.
pub(super) fn android_manifest() -> String {
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
     <manifest xmlns:android=\"http://schemas.android.com/apk/res/android\">\n\
     </manifest>\n"
        .to_string()
}

/// `platforms/android/src/main/java/HooksPlugin.kt`: the hand-written Kotlin seam.
pub(super) fn android_hooks_plugin_kt(plan: &ProjectPlan) -> String {
    format!(
        "package {package}\n\n\
         import android.app.Activity\n\
         import app.tauri.annotation.Command\n\
         import app.tauri.annotation.InvokeArg\n\
         import app.tauri.annotation.TauriPlugin\n\
         import app.tauri.plugin.JSObject\n\
         import app.tauri.plugin.Plugin\n\
         import app.tauri.plugin.Invoke\n\n\
         @InvokeArg\n\
         class PingArgs {{\n\
         \x20   var value: String? = null\n\
         }}\n\n\
         // @Command methods are called from Rust through the hooks plugin's `run_mobile_plugin`.\n\
         @TauriPlugin\n\
         class HooksPlugin(private val activity: Activity) : Plugin(activity) {{\n\
         \x20   @Command\n\
         \x20   fun ping(invoke: Invoke) {{\n\
         \x20       val args = invoke.parseArgs(PingArgs::class.java)\n\
         \x20       val ret = JSObject()\n\
         \x20       ret.put(\"value\", args.value ?: \"\")\n\
         \x20       invoke.resolve(ret)\n\
         \x20   }}\n\
         }}\n",
        package = android_package(plan),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn plan() -> ProjectPlan {
        ProjectPlan::new(
            "acme-shop",
            PathBuf::from("acme-shop"),
            &[crate::Surface::Native],
        )
    }

    #[test]
    fn the_icon_set_ships_real_container_formats() {
        let by_name: std::collections::HashMap<_, _> = ICON_FILES.iter().copied().collect();
        assert!(by_name["icon.icns"].starts_with(b"icns"));
        assert_eq!(&by_name["icon.ico"][..4], &[0, 0, 1, 0]);
        for png in ["icon.png", "32x32.png", "128x128.png", "128x128@2x.png"] {
            assert!(by_name[png].starts_with(&[0x89, b'P', b'N', b'G']), "{png}");
        }
    }

    #[test]
    fn crate_names_derive_from_the_app_crate() {
        let plan = plan();
        assert_eq!(native_crate_name(&plan), "main_native");
        assert_eq!(hooks_crate_name(&plan), "main-native-hooks");
        assert_eq!(hooks_lib_name(&plan), "main_native_hooks");
    }

    #[test]
    fn bundle_identifier_has_no_underscores_and_android_package_no_hyphens() {
        let plan = plan();
        assert_eq!(bundle_identifier(&plan), "com.acme-shop.main");
        assert_eq!(android_package(&plan), "com.acme_shop.main.hooks");
    }

    #[test]
    fn native_cargo_builds_the_mobile_capable_targets() {
        let toml = native_cargo_toml(&plan());
        assert!(toml.contains("crate-type = [\"staticlib\", \"cdylib\", \"rlib\"]"));
        assert!(toml.contains("features = [\"native\"]"));
        assert!(toml.contains("app = { path = \"..\", package = \"main\" }"));
        assert!(toml.contains("main-native-hooks = { path = \"hooks\" }"));
    }

    #[test]
    fn native_lib_wires_pages_actions_hooks_and_plugin() {
        let lib = native_lib_rs(&plan());
        assert!(lib.contains("include!(concat!(env!(\"OUT_DIR\"), \"/pages.rs\"))"));
        assert!(lib.contains("use app::register_actions;"));
        assert!(lib.contains(".hooks(platform_hooks::hooks())"));
        assert!(lib.contains(".plugin(main_native_hooks::init())"));
        assert!(lib.contains("#[cfg_attr(mobile, tauri::mobile_entry_point)]"));
        assert!(lib.contains(".customize(|builder| ..)"));
    }

    #[test]
    fn desktop_hooks_show_the_event_seam() {
        let hooks = native_desktop_hooks_rs("macos");
        assert!(
            hooks.contains("NativeEvent::MenuSelected { id }"),
            "{hooks}"
        );
        assert!(hooks.contains("on_exit_requested"), "{hooks}");
    }

    #[test]
    fn tauri_conf_is_valid_json_with_no_static_windows() {
        let conf = native_tauri_conf_json(&plan());
        let parsed: serde_json::Value = serde_json::from_str(&conf).expect("valid JSON");
        assert_eq!(parsed["identifier"], "com.acme-shop.main");
        assert_eq!(parsed["app"]["windows"].as_array().map(Vec::len), Some(0));
        assert_eq!(parsed["bundle"]["iOS"]["minimumSystemVersion"], "13.0");
    }

    #[test]
    fn tauri_conf_ships_a_locked_down_csp() {
        let conf = native_tauri_conf_json(&plan());
        let parsed: serde_json::Value = serde_json::from_str(&conf).expect("valid JSON");
        let csp = parsed["app"]["security"]["csp"]
            .as_str()
            .expect("csp is a string, not null");
        // The scheme (both spellings), the wasm bundle, Tauri IPC, and the
        // sync WebSocket are allowed; nothing else is.
        for source in [
            "default-src 'self' fundamental:",
            "http://fundamental.localhost",
            "'wasm-unsafe-eval'",
            "ipc: http://ipc.localhost",
            "ws://localhost:*",
            "wss:",
        ] {
            assert!(csp.contains(source), "missing {source} in: {csp}");
        }
    }

    #[test]
    fn ios_package_name_derives_from_the_app() {
        let package = ios_package_swift(&plan());
        assert!(package.contains("name: \"main-native-hooks\""), "{package}");
        assert!(!package.contains("tauri-plugin-hooks"), "{package}");
        // The first-build path caveat is in the file itself.
        assert!(package.contains("../.tauri/tauri-api"), "{package}");
        assert!(
            package.contains("Materialized by the Tauri CLI"),
            "{package}"
        );
    }

    #[test]
    fn capabilities_json_parses_and_targets_the_main_window() {
        let parsed: serde_json::Value =
            serde_json::from_str(&native_capabilities_json()).expect("valid JSON");
        assert_eq!(parsed["windows"][0], "main");
    }

    fn sync_plan() -> ProjectPlan {
        ProjectPlan::new(
            "acme-shop",
            PathBuf::from("acme-shop"),
            &[crate::Surface::Native, crate::Surface::Sync],
        )
    }

    #[test]
    fn generated_rust_files_parse() {
        for source in [
            native_lib_rs(&plan()),
            native_lib_rs(&sync_plan()),
            native_main_rs(&plan()),
            native_platform_hooks_rs(),
            native_desktop_hooks_rs("macos"),
            hooks_lib_rs(&plan()),
            hooks_build_rs(),
            native_build_rs(),
        ] {
            syn::parse_file(&source).unwrap_or_else(|e| panic!("{e}:\n{source}"));
        }
    }

    #[test]
    fn a_sync_app_hosts_the_database_and_mounts_device_slices() {
        let lib = native_lib_rs(&sync_plan());
        assert!(
            lib.contains(".sync_host(sync_host_config(), device_slices())"),
            "{lib}"
        );
        assert!(
            lib.contains("fundamental::mount_device_slices! {}"),
            "{lib}"
        );
        assert!(lib.contains("FUNDAMENTAL_SYNC_URL"), "{lib}");
        assert!(lib.contains("FUNDAMENTAL_SYNC_TOKEN"), "{lib}");
        // The default URL bakes the plan's sync port.
        assert!(lib.contains("ws://localhost:9100"), "{lib}");

        let toml = native_cargo_toml(&sync_plan());
        assert!(
            toml.contains("features = [\"native\", \"native-sync-host\"]"),
            "{toml}"
        );
    }

    #[test]
    fn without_sync_there_is_no_sync_host() {
        let lib = native_lib_rs(&plan());
        assert!(!lib.contains("sync_host"), "{lib}");
        assert!(!lib.contains("mount_device_slices"), "{lib}");
        assert!(
            native_cargo_toml(&plan()).contains("features = [\"native\"]"),
            "no sync feature on a plain native app"
        );
    }

    #[test]
    fn hooks_build_points_mobile_sources_at_platforms() {
        let build = hooks_build_rs();
        assert!(build.contains(".android_path(\"../platforms/android\")"));
        assert!(build.contains(".ios_path(\"../platforms/ios\")"));
    }
}
