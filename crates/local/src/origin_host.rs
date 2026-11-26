//! [`OriginHost`] — where under an instance's suffix an origin is served.

use std::fmt;

use fundamental_core::address::DnsLabel;

/// Where under an instance's suffix an origin is served, as `[local.origins]` writes its `host`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OriginHost {
    /// The suffix itself (no `host`): `acme.test`.
    Suffix,
    /// One label in front (`host = "www"`): `www.acme.test`.
    Label(DnsLabel),
    /// Every name one label below a label in front (`host = "*.preview"`), passed through as `Host`.
    EveryLabelUnder(DnsLabel),
}

impl OriginHost {
    /// The host as written, `None` for the suffix itself or when `written` is neither one DNS
    /// label nor `*.` and one.
    #[must_use]
    pub fn parse(written: Option<&str>) -> Option<Self> {
        match written {
            None => Some(Self::Suffix),
            Some(written) => match written.strip_prefix("*.") {
                Some(label) => DnsLabel::parse(label).map(Self::EveryLabelUnder),
                None => DnsLabel::parse(written).map(Self::Label),
            },
        }
    }

    /// What stands in front of the suffix: `www`, `*.preview`, or `None` for the suffix itself.
    #[must_use]
    pub fn in_front(&self) -> Option<String> {
        match self {
            Self::Suffix => None,
            Self::Label(label) => Some(label.as_str().to_owned()),
            Self::EveryLabelUnder(label) => Some(format!("*.{}", label.as_str())),
        }
    }
}

impl fmt::Display for OriginHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.in_front() {
            Some(front) => f.write_str(&front),
            None => f.write_str("the apex itself"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_the_suffix_one_label_or_every_label_under_one() {
        assert_eq!(OriginHost::parse(None), Some(OriginHost::Suffix));
        assert_eq!(
            OriginHost::parse(Some("www")).and_then(|host| host.in_front()),
            Some("www".to_owned())
        );
        let preview = OriginHost::parse(Some("*.preview")).expect("a wildcard level");
        assert!(matches!(preview, OriginHost::EveryLabelUnder(_)));
        assert_eq!(preview.in_front().as_deref(), Some("*.preview"));
    }

    #[test]
    fn a_host_deeper_than_one_wildcard_level_is_refused() {
        for refused in [
            "*",
            "*.",
            "*.*.preview",
            "a.b",
            "x.*.preview",
            "*preview",
            "*.Preview",
            "",
        ] {
            assert_eq!(OriginHost::parse(Some(refused)), None, "{refused}");
        }
    }
}
