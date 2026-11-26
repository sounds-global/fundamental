//! [`DnsLabel`] — one name label, in the one spelling this framework writes.

#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;

/// One lower-case DNS label: 1 to 63 letters, digits and inner hyphens.
///
/// Upper case is refused rather than folded, so one name has one spelling.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::DnsLabel;
///
/// assert_eq!(DnsLabel::parse("328-m1").map(|label| label.to_string()).as_deref(), Some("328-m1"));
/// assert!(DnsLabel::parse("Lane").is_none());
/// assert!(DnsLabel::parse("-a").is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DnsLabel(String);

impl DnsLabel {
    /// Parses `text` as one label, or `None` when it is not one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::is_label(text).then(|| Self(String::from(text)))
    }

    /// Whether `text` is one lower-case label.
    #[must_use]
    pub fn is_label(text: &str) -> bool {
        let bytes = text.as_bytes();
        (1..=63).contains(&bytes.len())
            && bytes.first().is_some_and(|byte| *byte != b'-')
            && bytes.last().is_some_and(|byte| *byte != b'-')
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
    }

    /// The label as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DnsLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for DnsLabel {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::DnsLabel;

    #[test]
    fn a_label_is_lower_case_letters_digits_and_inner_hyphens() {
        for label in ["a", "0", "acme", "328-m1", &"a".repeat(63)] {
            assert!(DnsLabel::is_label(label), "{label}");
        }
    }

    #[test]
    fn anything_else_is_not_a_label() {
        for text in [
            "",
            "-a",
            "a-",
            "A",
            "Acme",
            "a.b",
            "a_b",
            "a b",
            "é",
            "a}\n",
            &"a".repeat(64),
        ] {
            assert!(DnsLabel::parse(text).is_none(), "{text:?}");
        }
    }
}
