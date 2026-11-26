use std::collections::BTreeSet;
use std::fmt;

use fundamental_core::address::TestApex;

/// The development apexes one project serves under: never empty, each once, in name order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApexSet(BTreeSet<TestApex>);

impl ApexSet {
    /// The set of `apexes`, or `None` when there are none.
    #[must_use]
    pub fn new(apexes: impl IntoIterator<Item = TestApex>) -> Option<Self> {
        let set: BTreeSet<TestApex> = apexes.into_iter().collect();
        (!set.is_empty()).then_some(Self(set))
    }

    /// Every apex, in name order.
    pub fn iter(&self) -> impl Iterator<Item = &TestApex> {
        self.0.iter()
    }

    /// Whether `apex` is in the set.
    #[must_use]
    pub fn contains(&self, apex: &TestApex) -> bool {
        self.0.contains(apex)
    }

    /// Every name a root for these apexes permits, sorted: each apex and its lanes twin.
    #[must_use]
    pub fn permitted(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .0
            .iter()
            .flat_map(|apex| [apex.to_string(), apex.lanes()])
            .collect();
        names.sort();
        names
    }

    /// The first apex shared with `other`, if any.
    #[must_use]
    pub fn overlap<'a>(&'a self, other: &Self) -> Option<&'a TestApex> {
        self.0.iter().find(|apex| other.contains(apex))
    }
}

impl fmt::Display for ApexSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for apex in &self.0 {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            write!(f, "`{apex}`")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::address::TestApex;

    use super::ApexSet;

    fn apex(text: &str) -> TestApex {
        TestApex::parse(text).expect("an apex")
    }

    #[test]
    fn a_set_is_never_empty_and_holds_each_apex_once() {
        assert_eq!(ApexSet::new([]), None);
        let set = ApexSet::new([apex("b.test"), apex("a.test"), apex("b.test")]).expect("two");
        assert_eq!(set.to_string(), "`a.test`, `b.test`");
        let other = ApexSet::new([apex("b.test")]).expect("one");
        assert_eq!(set.overlap(&other).map(TestApex::as_str), Some("b.test"));
    }

    #[test]
    fn a_root_permits_each_apex_and_its_lanes_twin() {
        let set = ApexSet::new([apex("b.test"), apex("a.test")]).expect("two");
        assert_eq!(
            set.permitted(),
            ["a-lane.test", "a.test", "b-lane.test", "b.test"]
        );
    }
}
