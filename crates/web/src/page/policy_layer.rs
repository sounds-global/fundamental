use super::guard_spec::GuardSpec;
use super::page_policy::PagePolicy;

/// A policy as it sits in a chain, with the file that declared it.
pub(crate) struct PolicyLayer {
    pub(crate) file: &'static str,
    pub(crate) policy: PagePolicy<()>,
}

impl PolicyLayer {
    pub(crate) fn new<G>(file: &'static str, policy: PagePolicy<G>) -> Self {
        Self {
            file,
            policy: policy.erased(),
        }
    }

    pub(crate) fn guards(&self) -> impl Iterator<Item = &GuardSpec> {
        self.policy.guards.iter()
    }
}
