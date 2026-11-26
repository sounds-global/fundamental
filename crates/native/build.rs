//! Sets the `desktop` / `mobile` cfg flags from the target OS, as `tauri_build::build()` does.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(desktop)");
    println!("cargo::rustc-check-cfg=cfg(mobile)");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "ios" | "android" => println!("cargo:rustc-cfg=mobile"),
        // Anything else (including the BSDs) gets the desktop surface.
        _ => println!("cargo:rustc-cfg=desktop"),
    }

    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
}
