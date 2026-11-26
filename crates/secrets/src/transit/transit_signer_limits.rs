//! [`TransitSignerLimits`] — the bounds a Transit signer holds its callers to.

use std::time::Duration;

/// How long one signature may take and how many may be in flight at once.
///
/// Exceeding either is a retryable refusal, never a queue, so a slow custody service cannot grow memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitSignerLimits {
    /// The deadline for one describe or sign call.
    pub deadline: Duration,
    /// The most calls in flight at once, across every clone of the signer.
    pub max_in_flight: usize,
}

impl Default for TransitSignerLimits {
    fn default() -> Self {
        Self {
            deadline: Duration::from_secs(2),
            max_in_flight: 256,
        }
    }
}
