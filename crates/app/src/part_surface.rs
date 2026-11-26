use std::time::Duration;

use fundamental_runtime::{DrainPhase, StartFuture, Surface};

/// A surface an [`AppPart`](crate::AppPart) runs, boxed.
pub(crate) struct PartSurface(Box<dyn Surface>);

impl PartSurface {
    pub(crate) fn new(surface: Box<dyn Surface>) -> Self {
        Self(surface)
    }
}

impl Surface for PartSurface {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn drain_phase(&self) -> DrainPhase {
        self.0.drain_phase()
    }

    fn drain_budget(&self) -> Option<Duration> {
        self.0.drain_budget()
    }

    fn start(self: Box<Self>) -> StartFuture {
        self.0.start()
    }
}
