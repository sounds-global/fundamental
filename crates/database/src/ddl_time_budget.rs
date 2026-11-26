//! [`DDL_TIME_BUDGET`] — how long a contended schema statement may be retried.

use std::time::Duration;

/// The longest a DDL statement may spend being retried, however many attempts remain.
///
/// A time rather than a count, because under catalog contention one attempt can take seconds.
pub const DDL_TIME_BUDGET: Duration = Duration::from_secs(30);
