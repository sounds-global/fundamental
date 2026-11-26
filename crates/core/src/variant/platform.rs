//! The `platform` variant axis: the operating system a native surface runs on.

use crate::Variant;

/// Variant axis for a native surface's operating system.
///
/// Written by hand, as `#[derive(Variant)]` with `#[variant(value = "macos")]` would generate it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Platform {
    /// Microsoft Windows (desktop).
    Windows,
    /// Linux (desktop).
    Linux,
    /// Apple macOS (desktop).
    MacOs,
    /// Apple iOS and iPadOS (mobile).
    Ios,
    /// Android (mobile).
    Android,
}

impl Variant for Platform {
    const KEY: &'static str = "platform";
    const OPTIONS: &'static [Self] = &[
        Self::Windows,
        Self::Linux,
        Self::MacOs,
        Self::Ios,
        Self::Android,
    ];

    fn value(&self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::MacOs => "macos",
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }

    fn from_value(value: &str) -> Option<Self> {
        match value {
            "windows" => Some(Self::Windows),
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::MacOs),
            "ios" => Some(Self::Ios),
            "android" => Some(Self::Android),
            _ => None,
        }
    }
}

impl Platform {
    /// The form factor this operating system implies.
    pub fn device(&self) -> crate::Device {
        match self {
            Self::Windows | Self::Linux | Self::MacOs => crate::Device::Desktop,
            Self::Ios | Self::Android => crate::Device::Mobile,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Platform;
    use crate::{Device, Variant};

    #[test]
    fn key_is_platform() {
        assert_eq!(Platform::KEY, "platform");
    }

    #[test]
    fn options_list_all_five_operating_systems_in_declaration_order() {
        assert_eq!(
            Platform::OPTIONS,
            &[
                Platform::Windows,
                Platform::Linux,
                Platform::MacOs,
                Platform::Ios,
                Platform::Android,
            ]
        );
    }

    #[test]
    fn value_returns_each_options_stable_string() {
        assert_eq!(Platform::Windows.value(), "windows");
        assert_eq!(Platform::Linux.value(), "linux");
        assert_eq!(Platform::MacOs.value(), "macos");
        assert_eq!(Platform::Ios.value(), "ios");
        assert_eq!(Platform::Android.value(), "android");
    }

    #[test]
    fn from_value_round_trips_every_option() {
        for option in Platform::OPTIONS {
            assert_eq!(Platform::from_value(option.value()), Some(*option));
        }
    }

    #[test]
    fn from_value_rejects_unknown_casing_and_the_snake_cased_spelling() {
        assert_eq!(Platform::from_value("mac_os"), None);
        assert_eq!(Platform::from_value("MacOs"), None);
        assert_eq!(Platform::from_value("freebsd"), None);
        assert_eq!(Platform::from_value(""), None);
    }

    #[test]
    fn each_platform_maps_to_its_form_factor() {
        assert_eq!(Platform::Windows.device(), Device::Desktop);
        assert_eq!(Platform::Linux.device(), Device::Desktop);
        assert_eq!(Platform::MacOs.device(), Device::Desktop);
        assert_eq!(Platform::Ios.device(), Device::Mobile);
        assert_eq!(Platform::Android.device(), Device::Mobile);
    }
}
