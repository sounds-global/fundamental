//! The narrowed transaction cannot be committed by the effect it was lent to.

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

use fundamental_db::Transaction as _;

#[subscribe(stream = "DEPOSITS", consumer = "booker", inbox = "billing.book", isolation = Serializable)]
async fn book(
    tx: &mut EffectTransaction<'_>,
    event: &Inbound<DepositPaid>,
) -> Result<(), BookingError> {
    let _ = event;
    tx.commit().await?;
    Ok(())
}

fn main() {}
