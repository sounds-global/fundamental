//! A session against this engine reads its cluster metadata, or says nothing at all.
//!
//! It guards `CqlNode::config`'s `token_aware_routing(false)`: nextest prints a passing test's
//! output nowhere, so the check is an assertion. The initial metadata read is awaited inside the
//! session build, so this is deterministic.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::{Arc, Mutex};

use fundamental_cql::{Ddl, ddl};
use fundamental_test_containers::cql::{CqlKeyspace, CqlNode};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

/// The driver module that reads the cluster's metadata, and the only one this watches.
const READER: &str = "scylla::cluster::metadata";

/// What distinguishes *this* failure from any other the reader may report.
///
/// Narrowed to the table name: the reader's unrelated `MOVED_NODE` complaint recovers on its own.
const MISSING_TABLE: &str = "system_schema";

/// The session builder's complaint when schema fetching is off but the full fetch is on.
const HALF_OFF: &str = "Inconsistent schema metadata configuration";

/// The module the session builder reports its configuration complaints from.
const BUILDER: &str = "scylla::client::session";

#[tokio::test]
async fn a_session_and_a_schema_change_report_no_metadata_failure() {
    let complaints = Complaints::install();

    let node = CqlNode::shared().await.expect("a node");
    let keyspace = CqlKeyspace::mint(node).await.expect("a test keyspace");

    // A schema change, which triggers a refresh.
    keyspace
        .pool()
        .execute_ddl(&ddl!(
            "CREATE TABLE IF NOT EXISTS widgets (id text PRIMARY KEY)"
        ))
        .await
        .expect("a table");
    keyspace
        .pool()
        .execute_ddl(&Ddl::new("DROP TABLE IF EXISTS widgets"))
        .await
        .expect("the table dropped");

    let seen = complaints.seen();
    let asking_for_a_table = seen
        .iter()
        .filter(|line| line.contains(MISSING_TABLE))
        .count();
    assert_eq!(
        asking_for_a_table,
        0,
        "the driver failed to read metadata because of a schema table this engine does not \
         have; it is fetching a routing map it can never build. Everything the reader said:\n{}",
        seen.join("\n")
    );
    let half_off = seen.iter().filter(|line| line.contains(HALF_OFF)).count();
    assert_eq!(
        half_off,
        0,
        "schema fetching is off but the full fetch is still asked for, so every session warns \
         at boot. Everything the driver said:\n{}",
        seen.join("\n")
    );
}

/// Every warning or error the driver's metadata reader emitted, in order.
///
/// Process-global, because the reader runs on tasks this test never touches.
#[derive(Clone)]
struct Complaints {
    seen: Arc<Mutex<Vec<String>>>,
}

impl Complaints {
    fn install() -> Self {
        let complaints = Self {
            seen: Arc::new(Mutex::new(Vec::new())),
        };
        // The `fmt` layer too, so a fixture that fails to provision still logs.
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("warn,fundamental_test_containers=info"));
        tracing_subscriber::registry()
            .with(complaints.clone())
            .with(fmt::layer().with_test_writer().with_filter(filter))
            .try_init()
            .expect(
                "this test has to own the process's subscriber, or it would assert over a \
                 capture the driver never reached — so it must be the only test in this file, \
                 and it must install before anything touches the node",
            );
        complaints
    }

    fn seen(&self) -> Vec<String> {
        self.seen
            .lock()
            .expect("the capture is not poisoned")
            .clone()
    }
}

impl<S: Subscriber> Layer<S> for Complaints {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let metadata = event.metadata();
        // `Level`'s order runs ERROR < WARN, so this is "warn or worse".
        let watched =
            metadata.target().starts_with(READER) || metadata.target().starts_with(BUILDER);
        if !watched || *metadata.level() > Level::WARN {
            return;
        }
        let mut message = Message::default();
        event.record(&mut message);
        self.seen
            .lock()
            .expect("the capture is not poisoned")
            .push(format!(
                "  {} {}: {}",
                metadata.level(),
                metadata.target(),
                message.0
            ));
    }
}

/// The event's fields, rendered for the failure message.
#[derive(Default)]
struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        if field.name() == "message" {
            self.0.push_str(&format!("{value:?}"));
        } else {
            self.0.push_str(&format!("{}={value:?}", field.name()));
        }
    }
}
