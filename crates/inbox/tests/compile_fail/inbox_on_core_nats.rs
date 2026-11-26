//! The inbox needs a durable consumer to redeliver what it could not apply; core NATS has none.

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

#[subscribe(inbox = "billing.book", isolation = Serializable)]
async fn book(tx: &mut EffectTransaction<'_>, event: &Inbound<DepositPaid>) -> Result<(), BookingError> {
    let _ = (tx, event);
    Ok(())
}

fn main() {}
