//! A [`ReadinessProbe`] over a connection pool ([`PoolProbe`]).

use fundamental_core::{AnyError, ProbeFuture, ReadinessProbe};

use crate::pool::HealthCheck;

/// Reports a store's reachability as a named readiness probe.
///
/// Named by the caller, because a process holds several pools of one type. Bounded on
/// [`HealthCheck`] so YCQL pools are included.
pub struct PoolProbe<P> {
    name: &'static str,
    pool: P,
}

impl<P> PoolProbe<P> {
    /// Names a pool for readiness reporting.
    ///
    /// `name` reaches an operator-facing payload, so it never carries a credential or a host.
    pub const fn new(name: &'static str, pool: P) -> Self {
        Self { name, pool }
    }
}

impl<P> ReadinessProbe for PoolProbe<P>
where
    P: HealthCheck + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn check(&self) -> ProbeFuture<'_> {
        Box::pin(async move { self.pool.health_check().await.map_err(AnyError::new) })
    }
}
