/// One object a loader takes: an object of `namespace`, granted at exactly `relation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Requirement {
    /// The object's namespace, e.g. `team`.
    pub namespace: &'static str,
    /// The relation the object must have been granted at, e.g. `member`.
    pub relation: &'static str,
}
