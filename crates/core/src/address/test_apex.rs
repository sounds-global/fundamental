//! [`TestApex`] — the one-label `.test` name a project develops under.

#[cfg(not(feature = "std"))]
use alloc::{format, string::String};
use core::fmt;

use crate::address::DnsLabel;

/// A development apex: exactly one lower-case DNS label under `.test`, `acme.test`.
///
/// A label ending in `-lane` is reserved for the apex's lanes twin, `acme-lane.test`.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::TestApex;
///
/// let apex = TestApex::parse("winkel.test").unwrap();
/// assert_eq!(apex.as_str(), "winkel.test");
/// assert_eq!(apex.label(), "winkel");
/// assert_eq!(apex.lanes(), "winkel-lane.test");
/// for refused in ["Winkel.test", ".test", "test", "a.b.test", "winkel.nl", "winkel-lane.test"] {
///     assert!(TestApex::parse(refused).is_none(), "{refused}");
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TestApex(String);

impl TestApex {
    /// The reserved top-level name every apex lives under.
    pub const TLD: &'static str = "test";

    /// What every lanes twin's label ends in, and so what no apex's label may end in.
    pub const LANES: &'static str = "-lane";

    /// Parses `text` as an apex, or `None` when it is not one lower-case label followed by
    /// `.test`, the label ends in `-lane`, or it is too long for its lanes twin.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let label = text.strip_suffix(".test")?;
        (DnsLabel::is_label(label)
            && !label.ends_with(Self::LANES)
            && label.len() + Self::LANES.len() <= 63)
            .then(|| Self(String::from(text)))
    }

    /// The apex's lanes twin, `acme-lane.test`: the registrable name every lane of the
    /// project serves under, never the same site as the apex.
    #[must_use]
    pub fn lanes(&self) -> String {
        format!("{}{}.{}", self.label(), Self::LANES, Self::TLD)
    }

    /// The apex whose lanes twin is `text`: `acme-lane.test` is `acme.test`'s.
    #[must_use]
    pub fn of_lanes(text: &str) -> Option<Self> {
        let label = text.strip_suffix(".test")?.strip_suffix(Self::LANES)?;
        Self::parse(&format!("{label}.{}", Self::TLD))
    }

    /// The apex as written, `acme.test`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The label in front of `.test`, `acme`.
    #[must_use]
    pub fn label(&self) -> &str {
        self.0.strip_suffix(".test").unwrap_or(&self.0)
    }
}

impl fmt::Display for TestApex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for TestApex {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::TestApex;

    #[test]
    fn an_apex_is_one_label_under_test() {
        let apex = TestApex::parse("acme.test");
        assert_eq!(apex.as_ref().map(TestApex::as_str), Some("acme.test"));
        assert_eq!(apex.as_ref().map(TestApex::label), Some("acme"));
    }

    #[test]
    fn nesting_case_and_other_tlds_are_refused() {
        for text in [
            "test",
            ".test",
            "a.b.test",
            "Acme.test",
            "acme.TEST",
            "acme.test.",
            "acme.nl",
            "-a.test",
            "a_b.test",
            "a}\n.test",
            "",
        ] {
            assert!(TestApex::parse(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn a_lanes_twin_is_reserved_and_never_an_apex() {
        let apex = TestApex::parse("acme.test").expect("an apex");
        assert_eq!(apex.lanes(), "acme-lane.test");
        assert_eq!(TestApex::of_lanes("acme-lane.test"), Some(apex));
        assert_eq!(TestApex::parse("acme-lane.test"), None);
        assert_eq!(TestApex::of_lanes("acme.test"), None);
        assert_eq!(TestApex::of_lanes("acme-lane-lane.test"), None);
        let longest = "a".repeat(58);
        let twin = TestApex::parse(&format!("{longest}.test")).map(|apex| apex.lanes());
        assert_eq!(twin, Some(format!("{longest}-lane.test")));
        assert_eq!(TestApex::parse(&format!("{longest}a.test")), None);
    }
}
