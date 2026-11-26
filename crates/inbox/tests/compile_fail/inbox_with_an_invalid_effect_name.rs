//! An effect name is checked where it is written, by the same rule the inbox applies.

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

#[subscribe(stream = "DEPOSITS", consumer = "booker", inbox = "Billing Book", isolation = Serializable)]
async fn book(tx: &mut EffectTransaction<'_>, event: &Inbound<DepositPaid>) -> Result<(), BookingError> {
    let _ = (tx, event);
    Ok(())
}

fn main() {}
