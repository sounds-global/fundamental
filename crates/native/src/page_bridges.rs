use std::sync::Arc;

use crate::{ActionBridge, BridgeSet};

/// The half-built state between [`BridgeSet::serving_pages`] and
/// [`and_actions`](Self::and_actions).
///
/// Not a [`BridgeSet`], so a split set without its action bridge does not compile.
#[must_use = "a PageBridges is not a BridgeSet until .and_actions names the other half"]
pub struct PageBridges {
    pages: Arc<dyn ActionBridge>,
}

impl PageBridges {
    pub(crate) fn new(pages: Arc<dyn ActionBridge>) -> Self {
        Self { pages }
    }

    /// Names the bridge that serves actions, completing the set.
    pub fn and_actions(self, bridge: impl ActionBridge) -> BridgeSet {
        BridgeSet::from_parts(self.pages, Arc::new(bridge))
    }
}
