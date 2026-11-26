//! A dependency a process must reach before it can serve ([`ReadinessProbe`]).

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;

use crate::AnyError;

/// The future a [`ReadinessProbe`] check returns.
pub type ProbeFuture<'a> = Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + 'a>>;

/// One dependency whose reachability decides whether this process should be sent traffic.
///
/// Readiness, not liveness: a failure removes the process from rotation, never restarts it.
pub trait ReadinessProbe: Send + Sync + 'static {
    /// The name reported for this probe, used in the response body and in metrics.
    ///
    /// It names the dependency ("sql", "nats"), never a credential or a host.
    fn name(&self) -> &'static str;

    /// Checks the dependency.
    ///
    /// # Errors
    /// [`AnyError`] when the dependency cannot currently serve; it must not embed secrets.
    fn check(&self) -> ProbeFuture<'_>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysUp;

    impl ReadinessProbe for AlwaysUp {
        fn name(&self) -> &'static str {
            "always-up"
        }

        fn check(&self) -> ProbeFuture<'_> {
            Box::pin(async { Ok(()) })
        }
    }

    struct AlwaysDown;

    impl ReadinessProbe for AlwaysDown {
        fn name(&self) -> &'static str {
            "always-down"
        }

        fn check(&self) -> ProbeFuture<'_> {
            Box::pin(async { Err(AnyError::new(crate::validate::ValidationErrors::default())) })
        }
    }

    #[tokio::test]
    async fn probes_of_different_types_share_one_collection() {
        let probes: Vec<Box<dyn ReadinessProbe>> = vec![Box::new(AlwaysUp), Box::new(AlwaysDown)];

        let mut outcomes = Vec::new();
        for probe in &probes {
            outcomes.push((probe.name(), probe.check().await.is_ok()));
        }

        assert_eq!(outcomes, vec![("always-up", true), ("always-down", false)]);
    }
}
