//! Connection pool traits. `SqlPool` lives in a backend crate, so this sketch is not compiled:
//!
//! ```ignore
//! // Create a pool
//! let pool = SqlPool::connect(&config).await?;
//!
//! // Use the pool
//! let users = pool.repository::<User>();
//! let user = users.find(&user_id).await?;
//!
//! // Check pool status
//! let status = pool.status();
//! println!("Active connections: {}", status.active);
//! ```

mod close_reason;
mod config;
mod connection_pool;
mod event;
mod health_check;
mod listener;
mod pool_probe;
mod status;

pub use close_reason::CloseReason;
pub use config::PoolConfig;
pub use connection_pool::ConnectionPool;
pub use event::PoolEvent;
pub use health_check::HealthCheck;
pub use listener::PoolListener;
pub use pool_probe::PoolProbe;
pub use status::PoolStatus;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_pool_status() {
        let status = PoolStatus {
            active: Some(5),
            idle: Some(3),
            max: 10,
            min: Some(1),
            pending: Some(0),
        };

        assert_eq!(status.total(), Some(8));
        assert_eq!(status.utilization(), Some(50.0));
        assert_eq!(status.is_saturated(), Some(false));
        assert_eq!(status.has_capacity(), Some(true));
    }

    #[test]
    fn test_pool_config() {
        let config = PoolConfig::new("postgres://localhost/test")
            .min_connections(2)
            .max_connections(20)
            .acquire_timeout(Duration::from_secs(5));

        assert_eq!(config.min_connections, 2);
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.acquire_timeout, Duration::from_secs(5));
    }
}
