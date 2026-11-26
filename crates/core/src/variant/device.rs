//! The `device` variant axis: the form factor a native surface runs on.

use crate::Variant;

/// Variant axis for a native surface's form factor: desktop or mobile.
///
/// Written by hand, as `#[derive(Variant)]` would generate it: this crate has no derive dependency.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Device {
    /// A desktop or laptop computer (windowed, keyboard-and-pointer input).
    Desktop,
    /// A phone or tablet (full-screen, touch input).
    Mobile,
}

impl Variant for Device {
    const KEY: &'static str = "device";
    const OPTIONS: &'static [Self] = &[Self::Desktop, Self::Mobile];

    fn value(&self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Mobile => "mobile",
        }
    }

    fn from_value(value: &str) -> Option<Self> {
        match value {
            "desktop" => Some(Self::Desktop),
            "mobile" => Some(Self::Mobile),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Device;
    use crate::Variant;

    #[test]
    fn key_is_device() {
        assert_eq!(Device::KEY, "device");
    }

    #[test]
    fn options_list_both_form_factors_in_declaration_order() {
        assert_eq!(Device::OPTIONS, &[Device::Desktop, Device::Mobile]);
    }

    #[test]
    fn value_returns_each_options_stable_string() {
        assert_eq!(Device::Desktop.value(), "desktop");
        assert_eq!(Device::Mobile.value(), "mobile");
    }

    #[test]
    fn from_value_round_trips_every_option() {
        for option in Device::OPTIONS {
            assert_eq!(Device::from_value(option.value()), Some(*option));
        }
    }

    #[test]
    fn from_value_rejects_unknown_and_empty_strings() {
        assert_eq!(Device::from_value("tablet"), None);
        assert_eq!(Device::from_value("Desktop"), None);
        assert_eq!(Device::from_value(""), None);
    }
}
