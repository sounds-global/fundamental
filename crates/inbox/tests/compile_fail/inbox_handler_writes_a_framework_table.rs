//! A model over a `fundamental_` table is refused outside the framework, so no handler can rewrite the inbox's ledger.

use fundamental_db::DatabaseError;
use fundamental_inbox::{EffectTransaction, Inbound};
use fundamental_macros::{Error, mount_messages, subscribe};
use fundamental_nats::{EventId, Message};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Message)]
#[subject = "deposits.paid"]
struct DepositPaid {
    #[event_id]
    id: EventId,
    cents: i64,
}

#[derive(Debug, Error)]
enum BookingError {
    #[error(code = "BOOK_001", message = "Booking store failed", status = 503)]
    Store(#[from] DatabaseError),
}

mount_messages! { DepositPaid }

#[derive(Clone, Debug, fundamental_macros::Model)]
#[model(sql, table = "fundamental_inbox")]
struct Ledger {
    #[model(id)]
    effect: String,
}

fn main() {}
