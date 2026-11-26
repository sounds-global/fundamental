//! [`LocalSuffix`] — the names one checkout serves under while it is developed.

#[cfg(not(feature = "std"))]
use alloc::{format, string::String};
use core::fmt;

use crate::address::{DnsLabel, TestApex};

/// The suffix one checkout's development names end in: the apex itself for a project's main
/// checkout, `acme.test`, or `<label>.<apex's lanes twin>` for a lane,
/// `328-m1.acme-lane.test`.
///
/// A lane's names sit under the apex's lanes twin ([`TestApex::lanes`]), a registrable name
/// of its own, so a lane is never the same site as main.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::{DnsLabel, LocalSuffix, TestApex};
///
/// let apex = TestApex::parse("acme.test").unwrap();
/// let lane = LocalSuffix::lane(DnsLabel::parse("328-m1").unwrap(), apex.clone());
/// assert_eq!(lane.to_string(), "328-m1.acme-lane.test");
/// assert_eq!(lane.name(Some("www")), "www.328-m1.acme-lane.test");
/// assert_eq!(lane.site(), "acme-lane.test");
///
/// let (host, suffix) = LocalSuffix::of_host("www.328-m1.acme-lane.test").unwrap();
/// assert_eq!((host, suffix), (Some("www"), lane));
/// assert_eq!(LocalSuffix::of_host("www.acme.test"), Some((Some("www"), LocalSuffix::main(apex))));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalSuffix {
    apex: TestApex,
    lane: Option<DnsLabel>,
}

impl LocalSuffix {
    /// A main checkout's suffix: the apex itself.
    #[must_use]
    pub const fn main(apex: TestApex) -> Self {
        Self { apex, lane: None }
    }

    /// A lane's suffix, `<label>.<apex's lanes twin>`.
    #[must_use]
    pub const fn lane(label: DnsLabel, apex: TestApex) -> Self {
        Self {
            apex,
            lane: Some(label),
        }
    }

    /// Parses a written suffix — an apex, or `<label>.<apex's lanes twin>` — or `None` when
    /// `text` is neither.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(apex) = TestApex::parse(text) {
            return Some(Self::main(apex));
        }
        let (label, rest) = text.split_once('.')?;
        let apex = TestApex::of_lanes(rest)?;
        Some(Self::lane(DnsLabel::parse(label)?, apex))
    }

    /// Splits a development host into the host label in front of its suffix and the suffix,
    /// or `None` when `host` is no checkout's name.
    #[must_use]
    pub fn of_host(host: &str) -> Option<(Option<&str>, Self)> {
        if let Some(suffix) = Self::parse(host) {
            return Some((None, suffix));
        }
        let (label, rest) = host.split_once('.')?;
        if !DnsLabel::is_label(label) {
            return None;
        }
        Self::parse(rest).map(|suffix| (Some(label), suffix))
    }

    /// The apex this suffix is under.
    #[must_use]
    pub const fn apex(&self) -> &TestApex {
        &self.apex
    }

    /// The registrable name — the browser's site — this suffix's names belong to: the apex
    /// for main, its lanes twin for a lane.
    #[must_use]
    pub fn site(&self) -> String {
        match self.lane {
            Some(_) => self.apex.lanes(),
            None => format!("{}", self.apex),
        }
    }

    /// The lane's label, or `None` for a main checkout.
    #[must_use]
    pub fn lane_label(&self) -> Option<&str> {
        self.lane.as_ref().map(DnsLabel::as_str)
    }

    /// The name `host` has under this suffix, or the suffix itself when `host` is `None`.
    #[must_use]
    pub fn name(&self, host: Option<&str>) -> String {
        match host {
            Some(host) => format!("{host}.{self}"),
            None => format!("{self}"),
        }
    }

    /// The main checkout's twin of a development host: `mijn.328-m1.shop-lane.test` is
    /// `mijn.shop.test`, a main host is itself, and a host that is no checkout's is `None`.
    #[must_use]
    pub fn main_twin(host: &str) -> Option<String> {
        let (label, suffix) = Self::of_host(host)?;
        Some(Self::main(suffix.apex).name(label))
    }
}

impl fmt::Display for LocalSuffix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.lane {
            Some(label) => write!(f, "{label}.{}", self.apex.lanes()),
            None => write!(f, "{}", self.apex),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LocalSuffix;
    use crate::address::{DnsLabel, TestApex};

    fn apex() -> TestApex {
        TestApex::parse("acme.test").expect("a valid apex")
    }

    fn label(text: &str) -> DnsLabel {
        DnsLabel::parse(text).expect("a valid label")
    }

    #[test]
    fn main_and_lane_suffixes_are_written_and_read_back() {
        let main = LocalSuffix::main(apex());
        let lane = LocalSuffix::lane(label("328-m1"), apex());
        assert_eq!(main.to_string(), "acme.test");
        assert_eq!(lane.to_string(), "328-m1.acme-lane.test");
        assert_eq!(LocalSuffix::parse("acme.test"), Some(main));
        assert_eq!(LocalSuffix::parse("328-m1.acme-lane.test"), Some(lane));
    }

    #[test]
    fn a_main_name_and_a_lane_name_never_share_a_shape() {
        let lane = LocalSuffix::lane(label("grafana"), apex());
        let (host, suffix) = LocalSuffix::of_host("grafana.acme.test").expect("main's host");
        assert_eq!((host, suffix.lane_label()), (Some("grafana"), None));
        assert_ne!(suffix, lane);
        assert_eq!(lane.name(None), "grafana.acme-lane.test");
    }

    #[test]
    fn a_lane_is_never_the_same_site_as_main() {
        let main = LocalSuffix::main(apex());
        for text in ["328-m1", "main", "acme", "lane", "www"] {
            let lane = LocalSuffix::lane(label(text), apex());
            assert_ne!(lane.site(), main.site(), "{lane}");
            assert_eq!(registrable(&lane.name(Some("www"))), lane.site(), "{lane}");
            assert_ne!(registrable(&lane.name(Some("www"))), "acme.test", "{lane}");
        }
        assert_eq!(registrable(&main.name(Some("www"))), "acme.test");
    }

    /// The registrable domain a browser derives for a `.test` host, which the public suffix
    /// list does not name: the label in front of the top-level name.
    fn registrable(host: &str) -> String {
        let labels: Vec<&str> = host.rsplitn(3, '.').collect();
        format!("{}.{}", labels[1], labels[0])
    }

    #[test]
    fn a_host_that_is_no_checkouts_is_refused() {
        for host in [
            "a.b.acme.test",
            "x.lane.lane.lane.acme.test",
            "a.b.c.acme-lane.test",
            "acme-lane.test",
            "www.acme.nl",
            "Www.acme.test",
            "x.acme-lane.test.",
            "",
        ] {
            assert_eq!(LocalSuffix::of_host(host), None, "{host}");
        }
        assert_eq!(LocalSuffix::parse("Lane.acme-lane.test"), None);
        assert_eq!(LocalSuffix::parse("a.b.acme-lane.test"), None);
        assert_eq!(LocalSuffix::parse("a.acme-lane-lane.test"), None);
    }

    #[test]
    fn a_lanes_host_has_main_as_its_twin() {
        assert_eq!(
            LocalSuffix::main_twin("mijn.328-m1.shop-lane.test").as_deref(),
            Some("mijn.shop.test")
        );
        assert_eq!(
            LocalSuffix::main_twin("mijn.shop.test").as_deref(),
            Some("mijn.shop.test")
        );
        assert_eq!(LocalSuffix::main_twin("mijn.shop.nl"), None);
    }
}
