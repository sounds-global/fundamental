//! YCQL connection pool.

mod conn;
mod ops;

pub use conn::CqlPool;
pub use ops::OperationGuard;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use fundamental_db::PoolStatus;

    use super::*;

    #[test]
    fn test_pool_status_calculation() {
        let status = PoolStatus {
            active: Some(3),
            idle: Some(0),
            max: 3,
            min: Some(1),
            pending: Some(0),
        };
        assert_eq!(status.total(), Some(3));
    }

    #[test]
    fn test_operation_guard_no_warning() {
        let warnings = Arc::new(AtomicU64::new(0));
        let threshold = Some(Duration::from_secs(1));

        {
            let _guard = OperationGuard::new(threshold, Arc::clone(&warnings));
        }

        assert_eq!(warnings.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_operation_guard_with_warning() {
        let warnings = Arc::new(AtomicU64::new(0));
        let threshold = Some(Duration::from_millis(10));

        {
            let _guard = OperationGuard::new(threshold, Arc::clone(&warnings));
            std::thread::sleep(Duration::from_millis(20));
        }

        assert_eq!(warnings.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_operation_guard_disabled() {
        let warnings = Arc::new(AtomicU64::new(0));

        {
            let _guard = OperationGuard::new(None, Arc::clone(&warnings));
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(warnings.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_operation_guard_elapsed() {
        let warnings = Arc::new(AtomicU64::new(0));
        let guard = OperationGuard::new(None, warnings);

        std::thread::sleep(Duration::from_millis(5));

        assert!(guard.elapsed() >= Duration::from_millis(5));
    }

    #[test]
    fn test_operation_guard_with_name() {
        let warnings = Arc::new(AtomicU64::new(0));
        let threshold = Some(Duration::from_millis(10));

        {
            let _guard =
                OperationGuard::with_operation(threshold, Arc::clone(&warnings), "test_operation");
            std::thread::sleep(Duration::from_millis(20));
        }

        assert_eq!(warnings.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_operation_guard_multiple_warnings() {
        let warnings = Arc::new(AtomicU64::new(0));
        let threshold = Some(Duration::from_millis(5));

        for _ in 0..3 {
            let _guard = OperationGuard::new(threshold, Arc::clone(&warnings));
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(warnings.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_operation_guard_in_flight_tracking() {
        let warnings = Arc::new(AtomicU64::new(0));
        let in_flight = Arc::new(AtomicU64::new(0));
        let threshold = Some(Duration::from_secs(1));

        assert_eq!(in_flight.load(Ordering::Relaxed), 0);

        {
            let _guard = OperationGuard::with_in_flight(
                threshold,
                Arc::clone(&warnings),
                Arc::clone(&in_flight),
            );
            assert_eq!(in_flight.load(Ordering::Relaxed), 1);
        }

        assert_eq!(in_flight.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_operation_guard_in_flight_multiple() {
        let warnings = Arc::new(AtomicU64::new(0));
        let in_flight = Arc::new(AtomicU64::new(0));
        let threshold = None;

        let guard1 = OperationGuard::with_in_flight(
            threshold,
            Arc::clone(&warnings),
            Arc::clone(&in_flight),
        );
        assert_eq!(in_flight.load(Ordering::Relaxed), 1);

        let guard2 = OperationGuard::with_in_flight(
            threshold,
            Arc::clone(&warnings),
            Arc::clone(&in_flight),
        );
        assert_eq!(in_flight.load(Ordering::Relaxed), 2);

        let guard3 = OperationGuard::with_in_flight_named(
            threshold,
            Arc::clone(&warnings),
            Arc::clone(&in_flight),
            "test_op",
        );
        assert_eq!(in_flight.load(Ordering::Relaxed), 3);

        drop(guard1);
        assert_eq!(in_flight.load(Ordering::Relaxed), 2);

        drop(guard2);
        assert_eq!(in_flight.load(Ordering::Relaxed), 1);

        drop(guard3);
        assert_eq!(in_flight.load(Ordering::Relaxed), 0);
    }

    // `CqlPool::graceful_shutdown` is pinned in `tests/integration.rs`, against a real pool.
}
