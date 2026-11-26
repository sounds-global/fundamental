//! [`BackoffRejection`] — why [`Backoff::new`](super::Backoff::new) refused its parameters.

use core::fmt;
use core::time::Duration;

/// Why a schedule was refused: it could wait zero or shrink towards zero.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum BackoffRejection {
    /// The first window is shorter than [`Backoff::MIN_FIRST`](super::Backoff::MIN_FIRST).
    FirstTooShort {
        /// The first window asked for.
        first: Duration,
    },
    /// The growth is not a finite number of at least 1, so the window would shrink or vanish.
    GrowthBelowOne {
        /// The growth asked for.
        growth: f64,
    },
    /// The cap is below the first window, so it would narrow every draw below the first.
    CapBelowFirst {
        /// The first window asked for.
        first: Duration,
        /// The cap asked for.
        cap: Duration,
    },
}

impl fmt::Display for BackoffRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FirstTooShort { first } => write!(
                f,
                "the first window is {first:?}; it must be at least 1 ms, or every client redials at once"
            ),
            Self::GrowthBelowOne { growth } => write!(
                f,
                "the growth is {growth}; it must be a finite number of at least 1, or the window shrinks to nothing"
            ),
            Self::CapBelowFirst { first, cap } => {
                write!(f, "the cap {cap:?} is below the first window {first:?}")
            }
        }
    }
}

impl core::error::Error for BackoffRejection {}
