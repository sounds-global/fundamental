use std::fmt;

/// The browser's `Sec-Fetch-Site` signal, as a class; the value itself is never recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FetchSite {
    /// The request carried no `Sec-Fetch-Site`.
    Absent,
    /// `same-origin`.
    SameOrigin,
    /// `same-site`: a different origin on the same registrable domain.
    SameSite,
    /// `cross-site`.
    CrossSite,
    /// `none`: a navigation the user started, with no initiating site.
    None,
    /// A value the specification does not define.
    Unrecognised,
}

impl FetchSite {
    /// Classifies a `Sec-Fetch-Site` value; `None` is [`FetchSite::Absent`].
    #[must_use]
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Option::None => Self::Absent,
            Some("same-origin") => Self::SameOrigin,
            Some("same-site") => Self::SameSite,
            Some("cross-site") => Self::CrossSite,
            Some("none") => Self::None,
            Some(_) => Self::Unrecognised,
        }
    }

    /// The class as a fixed word — the value a log field records.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::SameOrigin => "same-origin",
            Self::SameSite => "same-site",
            Self::CrossSite => "cross-site",
            Self::None => "none",
            Self::Unrecognised => "unrecognised",
        }
    }
}

impl fmt::Display for FetchSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defined_values_are_recognised() {
        let cases = [
            (Some("same-origin"), FetchSite::SameOrigin),
            (Some("same-site"), FetchSite::SameSite),
            (Some("cross-site"), FetchSite::CrossSite),
            (Some("none"), FetchSite::None),
            (Option::None, FetchSite::Absent),
        ];
        for (value, expected) in cases {
            assert_eq!(FetchSite::parse(value), expected);
            if let Some(value) = value {
                assert_eq!(expected.as_str(), value);
            }
        }
    }

    #[test]
    fn anything_else_is_one_class_and_never_its_value() {
        for hostile in ["jan@example.nl", "Same-Origin", "", "\u{1b}[2J"] {
            let site = FetchSite::parse(Some(hostile));
            assert_eq!(site, FetchSite::Unrecognised);
            assert_eq!(site.to_string(), "unrecognised");
        }
    }
}
