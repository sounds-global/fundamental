//! RAII timing guards that record metrics when dropped.

mod timing;
mod transaction;

pub use timing::TimingGuard;
pub use transaction::TransactionTimingGuard;
