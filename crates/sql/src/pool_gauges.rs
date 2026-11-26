use fundamental_db::ConnectionPool;
use prometheus::core::{Collector, Desc};
use prometheus::proto::MetricFamily;
use prometheus::{IntGaugeVec, Opts};

use crate::SqlConnections;

/// Exports every SQL pool's saturation as gauges, computed when Prometheus asks.
///
/// A collector, not a polling task, so each value is the pool's at the scrape. It does no I/O,
/// its only label is the configured instance name, and an unmeasured field is an absent series.
pub struct SqlPoolGauges {
    connections: SqlConnections,
    /// Connections the pool holds, by what they are doing.
    connections_gauge: IntGaugeVec,
    /// The ceiling, so a dashboard can divide without the deployment's config.
    limit_gauge: IntGaugeVec,
    /// Callers queued behind a connection; its own family, as a waiter is not a connection.
    waiters_gauge: IntGaugeVec,
}

impl SqlPoolGauges {
    /// Builds the gauges for every pool in `connections`, unregistered.
    ///
    /// # Errors
    /// Returns [`prometheus::Error`] if a metric name or label set is invalid.
    pub fn new(namespace: &str, connections: SqlConnections) -> Result<Self, prometheus::Error> {
        Ok(Self {
            connections,
            connections_gauge: IntGaugeVec::new(
                Opts::new(
                    "sql_pool_connections",
                    "YSQL pool connections, by what they are doing",
                )
                .namespace(namespace),
                &["instance", "state"],
            )?,
            limit_gauge: IntGaugeVec::new(
                Opts::new(
                    "sql_pool_connections_max",
                    "The ceiling on connections a YSQL pool may hold",
                )
                .namespace(namespace),
                &["instance"],
            )?,
            waiters_gauge: IntGaugeVec::new(
                Opts::new(
                    "sql_pool_waiters",
                    "Callers waiting for a YSQL connection right now",
                )
                .namespace(namespace),
                &["instance"],
            )?,
        })
    }
}

impl Collector for SqlPoolGauges {
    fn desc(&self) -> Vec<&Desc> {
        self.connections_gauge
            .desc()
            .into_iter()
            .chain(self.limit_gauge.desc())
            .chain(self.waiters_gauge.desc())
            .collect()
    }

    fn collect(&self) -> Vec<MetricFamily> {
        // Reset first: a gauge vector keeps every label set, so a removed instance would report forever.
        self.connections_gauge.reset();
        self.limit_gauge.reset();
        self.waiters_gauge.reset();

        for (instance, pool) in self.connections.instances() {
            let status = pool.status();
            if let Some(active) = status.active {
                self.connections_gauge
                    .with_label_values(&[&instance, "active"])
                    .set(i64::from(active));
            }
            if let Some(idle) = status.idle {
                self.connections_gauge
                    .with_label_values(&[&instance, "idle"])
                    .set(i64::from(idle));
            }
            self.limit_gauge
                .with_label_values(&[&instance])
                .set(i64::from(status.max));
            if let Some(pending) = status.pending {
                self.waiters_gauge
                    .with_label_values(&[&instance])
                    .set(i64::from(pending));
            }
        }

        self.connections_gauge
            .collect()
            .into_iter()
            .chain(self.limit_gauge.collect())
            .chain(self.waiters_gauge.collect())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn empty() -> SqlConnections {
        SqlConnections::new(None, HashMap::new())
    }

    #[test]
    fn the_families_are_named_and_described() {
        let gauges = SqlPoolGauges::new("acme", empty()).expect("the gauges build");
        let names: Vec<String> = gauges
            .desc()
            .iter()
            .map(|desc| desc.fq_name.clone())
            .collect();
        assert_eq!(
            names,
            vec![
                "acme_sql_pool_connections",
                "acme_sql_pool_connections_max",
                "acme_sql_pool_waiters",
            ]
        );
    }

    #[test]
    fn a_registry_with_no_pools_collects_nothing() {
        let gauges = SqlPoolGauges::new("acme", empty()).expect("the gauges build");
        let collected: usize = gauges
            .collect()
            .iter()
            .map(|family| family.get_metric().len())
            .sum();
        assert_eq!(collected, 0);
    }

    /// `Collector` is synchronous, so this compiles only while `collect` needs no runtime.
    #[test]
    fn collecting_needs_no_async_runtime() {
        let gauges = SqlPoolGauges::new("acme", empty()).expect("the gauges build");
        let _ = gauges.collect();
    }
}
