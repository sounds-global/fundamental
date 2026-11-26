//! A worker runs `#[subscribe]` handlers only: `mount_workers!` naming a plain function must
//! not compile.
//!
//! `on_hello` is a handler; `greet` is not, so it has no `greet_worker` for `register_workers`
//! to start. Expected to fail — the paired `unsubscribed_worker.stderr` snapshot pins the error.

use fundamental_macros::{mount_messages, mount_workers, subscribe};
use fundamental_nats::{Message, NatsResult, ReceivedMessage};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Message)]
#[subject = "events.hello"]
struct Hello {
    name: String,
}

mount_messages! { Hello }

#[subscribe]
async fn on_hello(message: ReceivedMessage<Hello>) -> NatsResult<()> {
    let _ = message.payload.name;
    Ok(())
}

async fn greet(message: ReceivedMessage<Hello>) -> NatsResult<()> {
    let _ = message.payload.name;
    Ok(())
}

mount_workers! { on_hello, greet }

fn main() {}
