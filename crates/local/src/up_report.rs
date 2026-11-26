use crate::{InstanceFacts, LocalError};

/// What `up` did, and what is still missing for a browser to reach the instance.
#[derive(Debug)]
pub struct UpReport {
    /// The instance, as it now stands.
    pub facts: InstanceFacts,
    /// Whether this run issued a new leaf.
    pub issued: bool,
    /// Whether this run minted the project's root.
    pub minted: bool,
    /// What a browser still lacks: the route (`LOCAL_004`), the trust (`LOCAL_005`), or why the stores could not be asked.
    pub warnings: Vec<LocalError>,
}
