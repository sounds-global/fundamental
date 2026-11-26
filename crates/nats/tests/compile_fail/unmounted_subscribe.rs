//! Subscribing to a message that `mount_messages!` did not list must not compile.
//!
//! `Mounted` is mounted; `Unmounted` is not. The handler for `Unmounted` fails the
//! `MountedMessage` bound the `#[subscribe]` expansion asserts. Expected to fail — the
//! paired `unmounted_subscribe.stderr` snapshot pins the error.

use fundamental_macros::{mount_messages, subscribe};
use fundamental_nats::{Message, NatsResult, ReceivedMessage};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Message)]
#[subject = "events.mounted"]
struct Mounted {
    id: u64,
}

#[derive(Serialize, Deserialize, Message)]
#[subject = "events.unmounted"]
struct Unmounted {
    id: u64,
}

mount_messages! { Mounted }

#[subscribe]
async fn on_unmounted(message: ReceivedMessage<Unmounted>) -> NatsResult<()> {
    let _ = message.payload.id;
    Ok(())
}

fn main() {}
