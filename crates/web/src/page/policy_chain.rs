use std::marker::PhantomData;

use super::guard_state::GuardState;
use super::join::Join;
use super::open::Open;
use super::page_policy::PagePolicy;
use super::policy_layer::PolicyLayer;

/// The policies that apply to one route, outermost first; its guard state is the join of theirs.
#[must_use]
pub struct PolicyChain<G = Open> {
    pub(crate) layers: Vec<PolicyLayer>,
    state: PhantomData<fn() -> G>,
}

impl PolicyChain<Open> {
    /// The chain above the pages root: nothing declared.
    pub fn root() -> Self {
        Self {
            layers: Vec::new(),
            state: PhantomData,
        }
    }
}

impl<G: GuardState> PolicyChain<G> {
    /// This chain with the policy `file` declares added below everything already in it.
    pub fn then<H>(mut self, file: &'static str, policy: PagePolicy<H>) -> PolicyChain<G::Output>
    where
        H: GuardState,
        G: Join<H>,
    {
        self.layers.push(PolicyLayer::new(file, policy));
        PolicyChain {
            layers: self.layers,
            state: PhantomData,
        }
    }
}
