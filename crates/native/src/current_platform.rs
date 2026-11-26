use fundamental_core::Platform;

/// The [`Platform`] this binary was compiled for.
///
/// The BSDs and other desktop unixes report [`Platform::Linux`].
#[must_use]
pub fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "ios") {
        Platform::Ios
    } else if cfg!(target_os = "android") {
        Platform::Android
    } else {
        Platform::Linux
    }
}

#[cfg(test)]
mod tests {
    use super::current_platform;
    use fundamental_core::{Device, Platform, Variant};

    #[test]
    fn current_platform_matches_the_compile_target() {
        let platform = current_platform();
        assert_eq!(platform.device(), Device::Desktop);
        assert_eq!(Platform::from_value(platform.value()), Some(platform));
    }
}
