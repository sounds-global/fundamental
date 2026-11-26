use std::sync::{Arc, Mutex};

/// A per-request, shared sink for the success status a handler wants; never applied to an error.
#[derive(Debug, Clone, Default)]
pub struct ResponseStatus {
    // `None` means the default `200`; the last write wins.
    status: Arc<Mutex<Option<u16>>>,
}

impl ResponseStatus {
    /// Creates an empty sink — no status recorded, so the router defaults to `200`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the success `status`; a poisoned lock is a silent no-op.
    pub fn set(&self, status: u16) {
        if let Ok(mut slot) = self.status.lock() {
            *slot = Some(status);
        }
    }

    /// Reads the recorded status without consuming it, for an observer between handler and router.
    #[must_use]
    pub fn get(&self) -> Option<u16> {
        self.status.lock().ok().and_then(|slot| *slot)
    }

    /// Takes the recorded status; `None` means the default `200`.
    #[must_use]
    pub fn take(&self) -> Option<u16> {
        self.status.lock().ok().and_then(|mut slot| slot.take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_sink_takes_none() {
        assert_eq!(ResponseStatus::new().take(), None);
    }

    #[test]
    fn a_recorded_status_is_taken_once() {
        let sink = ResponseStatus::new();
        sink.set(202);
        assert_eq!(sink.take(), Some(202));
        assert_eq!(sink.take(), None, "take drains the sink");
    }

    #[test]
    fn the_last_write_wins() {
        let sink = ResponseStatus::new();
        sink.set(201);
        sink.set(202);
        assert_eq!(sink.take(), Some(202));
    }

    #[test]
    fn a_clone_shares_the_handle() {
        let handler_side = ResponseStatus::new();
        let router_side = handler_side.clone();
        handler_side.set(202);
        assert_eq!(router_side.take(), Some(202));
    }
}
