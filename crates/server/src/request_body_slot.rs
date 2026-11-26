use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use crate::RequestBodyStream;

/// The take-once slot an [`HttpContext`](crate::HttpContext) carries a streamed request body in.
#[derive(Clone, Default)]
pub(crate) struct RequestBodySlot {
    body: Arc<Mutex<Option<RequestBodyStream>>>,
}

impl RequestBodySlot {
    pub(crate) fn holding(body: RequestBodyStream) -> Self {
        Self {
            body: Arc::new(Mutex::new(Some(body))),
        }
    }

    pub(crate) fn take(&self) -> Option<RequestBodyStream> {
        self.body
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

impl fmt::Debug for RequestBodySlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RequestBodySlot")
    }
}
