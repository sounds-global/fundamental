use std::sync::Arc;

use crate::{ActionBridge, PageBridges};

/// The pair of [`ActionBridge`]s a native app dispatches through: one for pages,
/// one for actions.
///
/// Different halves come only through [`serving_pages`](Self::serving_pages) and
/// [`PageBridges::and_actions`], which cannot be transposed or left half-finished.
#[derive(Clone)]
pub struct BridgeSet {
    pages: Arc<dyn ActionBridge>,
    actions: Arc<dyn ActionBridge>,
}

impl BridgeSet {
    /// One bridge serving pages and actions.
    pub fn shared(bridge: impl ActionBridge) -> Self {
        let bridge: Arc<dyn ActionBridge> = Arc::new(bridge);
        Self {
            pages: Arc::clone(&bridge),
            actions: bridge,
        }
    }

    /// Begins a split set with the bridge that serves pages.
    pub fn serving_pages(bridge: impl ActionBridge) -> PageBridges {
        PageBridges::new(Arc::new(bridge))
    }

    pub(crate) fn from_parts(pages: Arc<dyn ActionBridge>, actions: Arc<dyn ActionBridge>) -> Self {
        Self { pages, actions }
    }

    /// The bridge serving pages and static assets.
    #[must_use]
    pub fn pages(&self) -> Arc<dyn ActionBridge> {
        Arc::clone(&self.pages)
    }

    /// The bridge serving actions.
    #[must_use]
    pub fn actions(&self) -> Arc<dyn ActionBridge> {
        Arc::clone(&self.actions)
    }

    /// Whether one bridge answers both halves.
    #[must_use]
    pub fn is_shared(&self) -> bool {
        Arc::ptr_eq(&self.pages, &self.actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BridgeRequest, BridgeResponse, NativeError};
    use fundamental_server::Method;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counting(Arc<AtomicUsize>);

    impl ActionBridge for Counting {
        fn call(
            &self,
            _request: BridgeRequest,
        ) -> Pin<Box<dyn Future<Output = Result<BridgeResponse, NativeError>> + Send + '_>>
        {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(BridgeResponse {
                    status: 200,
                    content_type: "application/json".to_string(),
                    headers: Vec::new(),
                    body: b"{}".to_vec(),
                })
            })
        }
    }

    #[test]
    fn a_shared_set_hands_one_bridge_to_both_halves() {
        let set = BridgeSet::shared(Counting(Arc::new(AtomicUsize::new(0))));
        assert!(set.is_shared());
        assert!(Arc::ptr_eq(&set.pages(), &set.actions()));
    }

    #[tokio::test]
    async fn a_split_set_keeps_its_halves_apart_and_in_the_order_they_were_named() {
        let pages = Arc::new(AtomicUsize::new(0));
        let actions = Arc::new(AtomicUsize::new(0));
        let set = BridgeSet::serving_pages(Counting(Arc::clone(&pages)))
            .and_actions(Counting(Arc::clone(&actions)));

        assert!(!set.is_shared());
        assert!(!Arc::ptr_eq(&set.pages(), &set.actions()));

        // Each half runs alone: a transposed pair is what this type makes unconstructible.
        set.pages()
            .call(BridgeRequest::new(Method::Get, "/page"))
            .await
            .expect("pages");
        assert_eq!(pages.load(Ordering::SeqCst), 1);
        assert_eq!(actions.load(Ordering::SeqCst), 0);

        set.actions()
            .call(BridgeRequest::new(Method::Post, "/action"))
            .await
            .expect("actions");
        assert_eq!(actions.load(Ordering::SeqCst), 1);
        assert_eq!(pages.load(Ordering::SeqCst), 1);
    }
}
