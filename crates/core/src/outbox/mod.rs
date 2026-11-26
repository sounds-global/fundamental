//! What a transactional outbox reports about the rows it could not relay.

mod drain_report;
mod held_count;
mod outbox_columns;
mod outbox_inventory;
mod refusal_kind;
mod sealed_outbox;
mod set_aside_bucket;
mod set_aside_tally;

pub use drain_report::DrainReport;
pub use held_count::HeldCount;
pub use outbox_columns::OutboxColumns;
pub use outbox_inventory::OutboxInventory;
pub use refusal_kind::RefusalKind;
pub use sealed_outbox::SealedOutbox;
pub use set_aside_bucket::SetAsideBucket;
pub use set_aside_tally::SetAsideTally;
