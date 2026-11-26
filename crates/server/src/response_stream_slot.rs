use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use crate::StreamedResponse;

/// The shared sink a streaming handler leaves its [`StreamedResponse`] in.
#[derive(Clone, Default)]
pub(crate) struct ResponseStreamSlot {
    body: Arc<Mutex<Option<StreamedResponse>>>,
}

impl ResponseStreamSlot {
    pub(crate) fn put(&self, body: StreamedResponse) {
        *self.body.lock().unwrap_or_else(PoisonError::into_inner) = Some(body);
    }

    pub(crate) fn take(&self) -> Option<StreamedResponse> {
        self.body
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

impl fmt::Debug for ResponseStreamSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResponseStreamSlot")
    }
}
