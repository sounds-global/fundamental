//! How a client waits before trying again: one full-jitter schedule, drawn from the OS CSPRNG.

mod backoff;
mod backoff_rejection;
mod entropy;
mod eventually;
#[cfg(any(test, feature = "seeded-entropy"))]
mod largest_bucket;
mod never_held;
#[cfg(any(test, feature = "seeded-entropy"))]
mod no_entropy;
mod os_entropy;
#[cfg(any(test, feature = "seeded-entropy"))]
mod seeded_entropy;

pub use backoff::Backoff;
pub use backoff_rejection::BackoffRejection;
pub use entropy::Entropy;
pub use eventually::eventually;
#[cfg(any(test, feature = "seeded-entropy"))]
pub use largest_bucket::largest_bucket;
pub use never_held::NeverHeld;
#[cfg(any(test, feature = "seeded-entropy"))]
pub use no_entropy::NoEntropy;
pub use os_entropy::OsEntropy;
#[cfg(any(test, feature = "seeded-entropy"))]
pub use seeded_entropy::SeededEntropy;
