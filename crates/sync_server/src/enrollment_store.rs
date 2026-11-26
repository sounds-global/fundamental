use std::future::Future;
use std::pin::Pin;

use fundamental_keys::KeyError;

use crate::DeviceEnrollment;

type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, KeyError>> + Send + 'a>>;

/// Stores device-key enrollments for [`EnrollmentRegistry`](crate::EnrollmentRegistry).
pub trait EnrollmentStore: Send + Sync {
    /// Returns every enrollment recorded for `device_id` (any status), or empty.
    fn load<'a>(&'a self, device_id: &'a str) -> StoreFuture<'a, Vec<DeviceEnrollment>>;

    /// Appends a new enrollment.
    fn insert(&self, enrollment: DeviceEnrollment) -> StoreFuture<'_, ()>;

    /// Moves every `Active` key of `device_id` to `Grace` with the given deadline.
    fn grace_active<'a>(&'a self, device_id: &'a str, grace_until: i64) -> StoreFuture<'a, ()>;

    /// Marks every key of `device_id` `Revoked` at `revoked_at`.
    fn revoke_all<'a>(&'a self, device_id: &'a str, revoked_at: i64) -> StoreFuture<'a, ()>;
}
