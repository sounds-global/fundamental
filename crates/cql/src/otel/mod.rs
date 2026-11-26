//! OpenTelemetry integration for YCQL tracing.
//!
//! ## Usage
//!
//! ```rust
//! use fundamental_cql::create_db_span;
//!
//! let span = create_db_span("select", "events", "my_keyspace");
//! let _guard = span.enter();
//!
//! // Execute the query within the span context, e.g.
//! // `pool.query::<Event>().partition_key(CqlValue::uuid(id)).execute().await?;`
//! ```

// The doctest above documents the re-exported `create_db_span`.
#![allow(rustdoc::private_doc_tests)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod fns;
mod timing;

pub use timing::SpanTimingGuard;

pub use fns::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_create_db_span() {
        let span = create_db_span("select", "events", "my_keyspace");
        let _guard = span.enter();
    }

    #[test]
    fn test_create_batch_span() {
        let span = create_batch_span("logged", "events", "my_keyspace", 10);
        let _guard = span.enter();
    }

    #[test]
    fn test_create_lwt_span() {
        let span = create_lwt_span("insert_if_not_exists", "users", "my_keyspace");
        let _guard = span.enter();
    }

    #[test]
    fn test_span_timing_guard() {
        let span = create_db_span("select", "events", "my_keyspace");
        let guard = SpanTimingGuard::new(span);
        std::thread::sleep(Duration::from_millis(5));
        let duration = guard.finish_success();
        assert!(duration.as_millis() >= 5);
    }

    #[test]
    fn test_record_outcome() {
        let span = create_db_span("select", "events", "my_keyspace");
        record_outcome(&span, true, None);
        record_outcome(&span, false, Some("connection error"));
    }

    #[test]
    fn test_record_lwt_result() {
        let span = create_lwt_span("insert_if_not_exists", "users", "my_keyspace");
        record_lwt_result(&span, true);
        record_lwt_result(&span, false);
    }
}
