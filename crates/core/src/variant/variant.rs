/// A typed axis of variation: a stable key plus its allowed options.
///
/// Implement it with `#[derive(Variant)]` on a fieldless enum; by hand it reads:
///
/// ```rust
/// use fundamental_core::Variant;
///
/// #[derive(Clone, Copy, PartialEq, Eq, Debug)]
/// enum Country {
///     Us,
///     Nl,
/// }
///
/// impl Variant for Country {
///     const KEY: &'static str = "country";
///     const OPTIONS: &'static [Self] = &[Self::Us, Self::Nl];
///     fn value(&self) -> &'static str {
///         match self {
///             Self::Us => "us",
///             Self::Nl => "nl",
///         }
///     }
///     fn from_value(value: &str) -> Option<Self> {
///         match value {
///             "us" => Some(Self::Us),
///             "nl" => Some(Self::Nl),
///             _ => None,
///         }
///     }
/// }
///
/// assert_eq!(Country::KEY, "country");
/// assert_eq!(Country::OPTIONS.len(), 2);
/// assert_eq!(Country::Us.value(), "us");
/// assert_eq!(Country::from_value("nl"), Some(Country::Nl));
/// assert_eq!(Country::from_value("xx"), None);
/// ```
pub trait Variant: Copy + Sized + 'static {
    /// Stable key identifying this axis (e.g. `"country"`), used to look up the active value in a context.
    const KEY: &'static str;

    /// All options for this axis, in declaration order.
    const OPTIONS: &'static [Self];

    /// This option's stable string value (e.g. `"us"`).
    fn value(&self) -> &'static str;

    /// Parse an option from its string value; `None` if unrecognized.
    fn from_value(value: &str) -> Option<Self>;
}

#[cfg(test)]
mod tests {
    use super::Variant;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Surface {
        Web,
        Native,
    }

    impl Variant for Surface {
        const KEY: &'static str = "surface";
        const OPTIONS: &'static [Self] = &[Self::Web, Self::Native];

        fn value(&self) -> &'static str {
            match self {
                Self::Web => "web",
                Self::Native => "native",
            }
        }

        fn from_value(value: &str) -> Option<Self> {
            match value {
                "web" => Some(Self::Web),
                "native" => Some(Self::Native),
                _ => None,
            }
        }
    }

    #[test]
    fn key_is_the_axis_name() {
        assert_eq!(Surface::KEY, "surface");
    }

    #[test]
    fn options_list_every_variant_in_declaration_order() {
        assert_eq!(Surface::OPTIONS, &[Surface::Web, Surface::Native]);
    }

    #[test]
    fn value_returns_each_options_stable_string() {
        assert_eq!(Surface::Web.value(), "web");
        assert_eq!(Surface::Native.value(), "native");
    }

    #[test]
    fn from_value_round_trips_every_option() {
        for option in Surface::OPTIONS {
            assert_eq!(Surface::from_value(option.value()), Some(*option));
        }
    }

    #[test]
    fn from_value_rejects_an_unknown_string() {
        assert_eq!(Surface::from_value("desktop"), None);
        assert_eq!(Surface::from_value(""), None);
    }
}
