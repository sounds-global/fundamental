//! Transactional outbox: enqueue a side effect atomically with its state change, then relay
//! it from a supervised drain.
//!
//! Delivery is effectively-once with an idempotent relay, and retries are scheduled by
//! [`RetryPolicy`], so a failing row cannot block the queue.

mod drain;
mod enqueue;
mod outbox_claim;
mod outbox_claimer;
mod outbox_row;
mod outbox_set_aside;
mod relay;
mod retry_policy;

pub use drain::drain_pending;
pub use enqueue::OutboxEnqueue;
pub use fundamental_core::outbox::{
    DrainReport, HeldCount, OutboxColumns, OutboxInventory, RefusalKind, SetAsideBucket,
    SetAsideTally,
};
pub use outbox_claim::OutboxClaim;
pub use outbox_claimer::OutboxClaimer;
pub use outbox_row::OutboxRow;
pub use outbox_set_aside::OutboxSetAside;
pub use relay::Relay;
pub use retry_policy::RetryPolicy;
