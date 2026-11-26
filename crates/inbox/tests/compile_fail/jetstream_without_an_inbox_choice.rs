//! A JetStream consumer is delivered at least once, so it must say whether the inbox guards its effect, or why it needs none.

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

use fundamental_nats::{NatsResult, ReceivedMessage};

#[subscribe(stream = "DEPOSITS", consumer = "booker", auto_ack)]
async fn book(message: ReceivedMessage<DepositPaid>) -> NatsResult<()> {
    let _ = message;
    Ok(())
}

fn main() {}
