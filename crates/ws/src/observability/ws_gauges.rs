//! [`WsGauges`]: a server's connections and closes, exported when Prometheus asks.

use std::sync::Arc;

use prometheus::core::{Collector, Desc};
use prometheus::proto::MetricFamily;
use prometheus::{IntCounterVec, IntGaugeVec, Opts};

use crate::observability::WsStats;
use crate::session::CloseReason;
use crate::slots::Pool;

/// Exports a server's [`WsStats`] at scrape time as an unregistered [`Collector`]; every label value is
/// from a closed set the server owns.
pub struct WsGauges {
    stats: Arc<WsStats>,
    connections: IntGaugeVec,
    connections_max: IntGaugeVec,
    closes: IntCounterVec,
    refusal_warnings: IntCounterVec,
}

impl WsGauges {
    pub(crate) fn new(namespace: &str, stats: Arc<WsStats>) -> Result<Self, prometheus::Error> {
        Ok(Self {
            stats,
            connections: IntGaugeVec::new(
                Opts::new(
                    "ws_connections",
                    "WebSocket connections open, by whether they have authenticated",
                )
                .namespace(namespace),
                &["state"],
            )?,
            connections_max: IntGaugeVec::new(
                Opts::new(
                    "ws_connections_max",
                    "The ceiling on WebSocket connections in each state",
                )
                .namespace(namespace),
                &["state"],
            )?,
            closes: IntCounterVec::new(
                Opts::new("ws_closes_total", "WebSocket connections closed, by reason")
                    .namespace(namespace),
                &["reason"],
            )?,
            refusal_warnings: IntCounterVec::new(
                Opts::new(
                    "ws_refusal_warnings_total",
                    "Refused-frame warnings, by whether a budget let them reach the log",
                )
                .namespace(namespace),
                &["outcome"],
            )?,
        })
    }
}

impl Collector for WsGauges {
    fn desc(&self) -> Vec<&Desc> {
        self.connections
            .desc()
            .into_iter()
            .chain(self.connections_max.desc())
            .chain(self.closes.desc())
            .chain(self.refusal_warnings.desc())
            .collect()
    }

    fn collect(&self) -> Vec<MetricFamily> {
        self.connections.reset();
        self.connections_max.reset();
        self.closes.reset();
        self.refusal_warnings.reset();
        for pool in Pool::ALL {
            let capacity = self.stats.capacity(pool);
            if capacity == 0 {
                continue;
            }
            self.connections
                .with_label_values(&[pool.label()])
                .set(saturating_i64(self.stats.open(pool)));
            self.connections_max
                .with_label_values(&[pool.label()])
                .set(saturating_i64(capacity));
        }
        for reason in CloseReason::ALL {
            self.closes
                .with_label_values(&[reason.label()])
                .inc_by(self.stats.closes(reason));
        }
        self.refusal_warnings
            .with_label_values(&["logged"])
            .inc_by(self.stats.refusal_warnings_logged());
        self.refusal_warnings
            .with_label_values(&["suppressed"])
            .inc_by(self.stats.refusal_warnings_suppressed());

        let mut families = self.connections.collect();
        families.extend(self.connections_max.collect());
        families.extend(self.closes.collect());
        families.extend(self.refusal_warnings.collect());
        families
    }
}

fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::{Encoder, Registry, TextEncoder};

    fn render(gauges: WsGauges) -> String {
        let registry = Registry::new();
        registry.register(Box::new(gauges)).expect("registered");
        let mut out = Vec::new();
        TextEncoder::new()
            .encode(&registry.gather(), &mut out)
            .expect("encoded");
        String::from_utf8(out).expect("utf-8")
    }

    #[test]
    fn ws_gauges_collect_without_a_runtime() {
        let stats = Arc::new(WsStats::new(2, 8, 0));
        stats.opened(Pool::Anonymous);
        stats.opened(Pool::Authenticated);
        stats.opened(Pool::Authenticated);
        stats.closed(CloseReason::AuthDeadline);
        stats.refusal_logged();
        stats.refusal_suppressed();
        stats.refusal_suppressed();
        let text = render(WsGauges::new("app", stats).expect("gauges"));
        assert!(
            text.contains(r#"app_ws_connections{state="anonymous"} 1"#),
            "{text}"
        );
        assert!(
            text.contains(r#"app_ws_connections{state="authenticated"} 2"#),
            "{text}"
        );
        assert!(
            text.contains(r#"app_ws_connections_max{state="authenticated"} 8"#),
            "{text}"
        );
        assert!(
            text.contains(r#"app_ws_closes_total{reason="auth_deadline"} 1"#),
            "{text}"
        );
        assert!(
            text.contains(r#"app_ws_closes_total{reason="displaced"} 0"#),
            "{text}"
        );
        assert!(
            text.contains(r#"app_ws_refusal_warnings_total{outcome="suppressed"} 2"#),
            "{text}"
        );
        assert!(
            !text.contains(r#"state="public""#),
            "a pool with no capacity is absent"
        );
    }

    #[test]
    fn a_second_scrape_reports_the_same_totals_not_their_sum() {
        let stats = Arc::new(WsStats::new(1, 1, 0));
        stats.closed(CloseReason::SlowPeer);
        let gauges = WsGauges::new("app", stats).expect("gauges");
        gauges.collect();
        let families = gauges.collect();
        let closes = families
            .iter()
            .find(|family| family.get_name() == "app_ws_closes_total")
            .expect("closes family");
        let slow = closes
            .get_metric()
            .iter()
            .find(|metric| metric.get_label()[0].get_value() == "slow_peer")
            .expect("slow_peer series");
        assert_eq!(slow.get_counter().get_value() as u64, 1);
    }
}
