//! Where a process is in its lifecycle ([`ServiceState`]).

#[cfg(not(feature = "std"))]
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU8, Ordering};
#[cfg(feature = "std")]
use std::sync::Arc;

/// Lifecycle phase, shared cheaply between a runtime and whatever reports on it.
///
/// Draining is published the instant shutdown begins, so a load balancer stops sending new work.
#[derive(Debug, Clone)]
pub struct ServiceState {
    phase: Arc<AtomicU8>,
}

/// The lifecycle phase a [`ServiceState`] currently holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Surfaces are still starting; the process is not yet ready for traffic.
    Starting,
    /// Every surface started; the process is serving.
    Ready,
    /// Shutdown has begun. Still serving what it accepted, accepting nothing new.
    Draining,
}

impl Phase {
    /// The wire encoding, monotonic so a stale read never moves a process backwards.
    const fn code(self) -> u8 {
        match self {
            Self::Starting => 0,
            Self::Ready => 1,
            Self::Draining => 2,
        }
    }

    const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Ready,
            2 => Self::Draining,
            // An unknown encoding is not-yet-serving: fail closed.
            _ => Self::Starting,
        }
    }

    /// The phase name, for logs and probe payloads.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Draining => "draining",
        }
    }
}

impl ServiceState {
    /// Creates a state in [`Phase::Starting`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            phase: Arc::new(AtomicU8::new(Phase::Starting.code())),
        }
    }

    /// The current phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        Phase::from_code(self.phase.load(Ordering::Acquire))
    }

    /// Marks the process ready to serve.
    ///
    /// Ignored once draining: shutdown is terminal.
    pub fn set_ready(&self) {
        let _ = self.phase.compare_exchange(
            Phase::Starting.code(),
            Phase::Ready.code(),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// Marks the process as draining. Terminal, and safe to call more than once.
    pub fn set_draining(&self) {
        self.phase.store(Phase::Draining.code(), Ordering::Release);
    }

    /// Whether the process should be sent new work.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.phase() == Phase::Ready
    }

    /// Whether the process is shutting down.
    #[must_use]
    pub fn is_draining(&self) -> bool {
        self.phase() == Phase::Draining
    }
}

impl Default for ServiceState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_state_is_not_yet_ready() {
        let state = ServiceState::new();
        assert_eq!(state.phase(), Phase::Starting);
        assert!(!state.is_ready());
        assert!(!state.is_draining());
    }

    #[test]
    fn readiness_is_published_then_withdrawn_on_drain() {
        let state = ServiceState::new();
        state.set_ready();
        assert!(state.is_ready());

        state.set_draining();
        assert!(state.is_draining());
        assert!(
            !state.is_ready(),
            "a draining process must stop being sent work"
        );
    }

    #[test]
    fn a_late_start_cannot_undo_draining() {
        let state = ServiceState::new();
        state.set_draining();
        state.set_ready();

        assert!(state.is_draining());
        assert!(!state.is_ready());
    }

    #[test]
    fn clones_share_one_phase() {
        let state = ServiceState::new();
        let observer = state.clone();
        state.set_ready();

        assert!(observer.is_ready(), "a clone observes the same process");
    }
}
