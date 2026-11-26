//! [`SegmentText`] — any text, escaped to stand as one segment of a link.

#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};

use crate::address::{PathSegment, percent_encode_segment};

/// Any text as one segment of a link — a tab's name, a search term — percent-encoded, so a `/`
/// or `?` in it can never restructure the URL it is written into.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SegmentText(String);

impl SegmentText {
    /// `text`, escaped. `None` for the empty text, which is no segment.
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        (!text.is_empty()).then(|| Self(percent_encode_segment(text).to_string()))
    }
}

impl PathSegment for SegmentText {
    fn to_segment(&self) -> String {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::SegmentText;
    use crate::address::PathSegment;

    #[test]
    fn a_separator_can_never_restructure_the_link() {
        let text = SegmentText::new("a/b?c#d").map(|text| text.to_segment());
        assert_eq!(text.as_deref(), Some("a%2Fb%3Fc%23d"));
        assert_eq!(SegmentText::new(""), None);
    }
}
