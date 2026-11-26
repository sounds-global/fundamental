//! OpenTelemetry tracing helpers for YSQL operations.

mod fns;
mod timing;

pub use timing::SpanTimingGuard;

pub use fns::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_create_query_span() {
        let span = create_query_span("select", "users", "myapp");
        let _guard = span.enter();
    }

    #[test]
    fn test_create_transaction_span() {
        let span = create_transaction_span("begin", "myapp");
        let _guard = span.enter();
    }

    #[test]
    fn test_create_batch_span() {
        let span = create_batch_span("insert", "users", "myapp", 10);
        let _guard = span.enter();
    }

    #[test]
    fn test_span_timing_guard() {
        let span = create_query_span("select", "users", "myapp");
        let guard = SpanTimingGuard::new(span);
        std::thread::sleep(Duration::from_millis(5));
        let duration = guard.finish_success();
        assert!(duration.as_millis() >= 5);
    }

    #[test]
    fn test_record_outcome() {
        let span = create_query_span("select", "users", "myapp");
        record_outcome(&span, true, None);
        record_outcome(&span, false, Some("test error"));
    }
}
