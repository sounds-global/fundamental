use std::future::Future;
use std::pin::Pin;

use crate::{BridgeRequest, BridgeResponse, NativeError};

/// Where a webview action call runs: the seam between UI and logic.
///
/// [`InProcessBridge`](crate::InProcessBridge) dispatches on an embedded router;
/// [`RemoteBridge`](crate::RemoteBridge) forwards over HTTPS.
pub trait ActionBridge: Send + Sync + 'static {
    /// Runs one action call to completion.
    ///
    /// # Errors
    /// Returns a [`NativeError`] only for transport-level failures; an application failure is
    /// an `Ok` response carrying its status and error envelope.
    fn call(
        &self,
        request: BridgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<BridgeResponse, NativeError>> + Send + '_>>;
}
