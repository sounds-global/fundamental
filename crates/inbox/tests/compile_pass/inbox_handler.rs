//! The inbox shape compiles: the handler writes the app's own model through the narrowed
//! transaction, and its listener takes the broker and the inbox.

use fundamental_db::DatabaseError;
use fundamental_db::RepositoryTransaction as _;
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
#[model(sql)]
struct Booking {
    #[model(id)]
    booking_id: uuid::Uuid,
    cents: i64,
}

#[subscribe(stream = "DEPOSITS", consumer = "booker", inbox = "billing.book", isolation = Serializable)]
async fn book(tx: &mut EffectTransaction<'_>, event: &Inbound<DepositPaid>) -> Result<(), BookingError> {
    tx.insert(&Booking {
        booking_id: event.event_id().as_uuid(),
        cents: event.payload().cents,
    })
    .await?;
    Ok(())
}

#[subscribe(stream = "DEPOSITS", consumer = "auditor", without_inbox = "an audit line keyed by its event id")]
async fn audit(message: fundamental_nats::ReceivedMessage<DepositPaid>) -> fundamental_nats::NatsResult<()> {
    let _ = message;
    Ok(())
}

#[allow(dead_code)]
async fn start(jetstream: &fundamental_nats::JetStream, inbox: &fundamental_inbox::Inbox) {
    let _ = book_listener(jetstream, inbox).await;
    let _ = audit_listener(jetstream).await;
}

fn main() {}
