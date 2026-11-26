//! `#[respond]` expands to the expected `{fn}_responder` surface.

use std::sync::Arc;

use fundamental_macros::{Schema, mount_responders, respond};
use fundamental_nats::{
    Listener, Message, MessageTransport, NatsClient, NatsResult, Queue, RespondMeta,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Message, Schema)]
#[subject = "test.rpc"]
struct Ping {
    n: u64,
}

#[derive(Serialize, Deserialize, Message, Schema)]
#[subject = "test.rpc.reply"]
struct Pong {
    n: u64,
}

/// The app state the handler reads.
struct Adder {
    base: u64,
}

/// A typed queue-group handle.
#[derive(Queue)]
#[queue(name = "adders")]
struct Adders;

#[respond(queue = Adders)]
async fn add(state: &Adder, request: Ping) -> Pong {
    Pong {
        n: state.base + request.n,
    }
}

#[respond(subject = "test.rpc.explicit")]
async fn add_explicit(state: &Adder, request: Ping) -> Pong {
    Pong {
        n: state.base + request.n,
    }
}

/// A handler that also reads the delivery metadata.
#[respond(subject = "test.rpc.metered")]
async fn add_metered(state: &Adder, request: Ping, meta: &RespondMeta) -> Pong {
    // A missing header is `None`, never a panic.
    let bonus = u64::from(meta.bearer_token().is_some());
    Pong {
        n: state.base + request.n + bonus,
    }
}

/// Pins the exact responder surface; it never runs.
async fn _signature(client: Arc<NatsClient>, state: Arc<Adder>) -> NatsResult<Listener> {
    let listener: Listener = add_responder(Arc::clone(&client), Arc::clone(&state)).await?;
    let _explicit: Listener = add_explicit_responder(client, state).await?;
    Ok(listener)
}

#[test]
fn respond_generates_responders_and_keeps_the_handler_pure() {
    let _ = add;
    let _ = add_responder;
    let _ = add_explicit_responder;
    // `mount_responders!` names only its descriptor, so this keeps it from being dead code.
    let _ = add_metered;
}

mount_responders! { add, add_explicit, add_metered }

#[test]
fn respond_emits_a_descriptor_carrying_request_and_reply_schemas() {
    let descriptor = add_descriptor();
    assert_eq!(descriptor.subject, "test.rpc");
    assert_eq!(descriptor.transport, MessageTransport::Nats);
    assert!(descriptor.is_request_reply());
    assert_eq!(descriptor.transport_label(), "nats_rpc");
    assert!(!matches!(
        descriptor.payload,
        fundamental_nats::SchemaNode::Any
    ));
    assert!(descriptor.reply.is_some());
    assert_eq!(descriptor.queue_group, Some("adders"));
}

/// A responder that names no group subscribes plainly, and says so.
#[test]
fn a_responder_with_no_queue_group_describes_none() {
    assert_eq!(add_explicit_descriptor().queue_group, None);
}

#[test]
fn mount_responders_collects_every_listed_responder() {
    let descriptors = describe_responders();
    let subjects: Vec<&str> = descriptors.iter().map(|d| d.subject).collect();
    assert_eq!(
        subjects,
        vec!["test.rpc", "test.rpc.explicit", "test.rpc.metered"]
    );
    assert!(descriptors.iter().all(|d| d.is_request_reply()));
}

/// A handle with no `name` override.
#[derive(Queue)]
struct LogWriters;

#[test]
fn queue_group_handle_carries_its_name() {
    assert_eq!(<Adders as Queue>::NAME, "adders");
    assert_eq!(<LogWriters as Queue>::NAME, "log_writers");
}
