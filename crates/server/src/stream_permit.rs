use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One body's place among a route's [`max_in_flight`](crate::StreamBounds::with_max_in_flight), returned when dropped.
#[derive(Debug)]
pub struct StreamPermit {
    in_flight: Arc<AtomicUsize>,
}

impl StreamPermit {
    pub(crate) fn holding(in_flight: Arc<AtomicUsize>) -> Self {
        Self { in_flight }
    }
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}
