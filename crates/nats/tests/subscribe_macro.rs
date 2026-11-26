//! `#[subscribe]` expands each transport variant to a `{fn}_listener` with the right client.

use fundamental_macros::{mount_messages, mount_workers, subscribe};
use fundamental_nats::{
    Consumer, JetStream, Message, MessageSet, NatsClient, NatsResult, Queue, ReceivedMessage,
    Stream, WorkerContext,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Message)]
#[subject = "events.user.created"]
struct UserCreated {
    user_id: String,
}

#[derive(Serialize, Deserialize, Message)]
#[subject = "events.user.activity"]
struct UserActivity {
    kind: String,
}

#[derive(Serialize, Deserialize, Message)]
#[subject = "orders.created"]
struct Order {
    id: u64,
}

// `#[subscribe]` requires each handler's payload to be mounted.
mount_messages! { UserCreated, UserActivity, Order; sets: AppEvents }

#[subscribe]
async fn on_user(message: ReceivedMessage<UserCreated>) -> NatsResult<()> {
    let _ = message.payload.user_id;
    Ok(())
}

#[subscribe(subject = "events.user.>", queue = "activity-workers")]
async fn on_user_activity(message: ReceivedMessage<UserActivity>) -> NatsResult<()> {
    let _ = message.payload.kind;
    Ok(())
}

#[subscribe(
    stream = "ORDERS",
    consumer = "order-processor",
    auto_ack,
    without_inbox = "an order is keyed by its id, so a redelivery overwrites itself"
)]
async fn on_order(message: ReceivedMessage<Order>) -> NatsResult<()> {
    let _ = message.payload.id;
    Ok(())
}

/// Typed JetStream stream handle.
#[derive(Stream)]
#[stream(name = "ORDERS")]
struct Orders;

/// Typed JetStream consumer handle; its `NAME` defaults to the snake_case type name.
#[derive(Consumer)]
struct OrderProcessor;

/// Typed queue-group handle for a core-NATS subscription.
#[derive(Queue)]
struct ActivityWorkers;

// JetStream with typed handles for both stream and consumer.
#[subscribe(
    stream = Orders,
    consumer = OrderProcessor,
    auto_ack,
    without_inbox = "an order is keyed by its id, so a redelivery overwrites itself"
)]
async fn on_order_typed(message: ReceivedMessage<Order>) -> NatsResult<()> {
    let _ = message.payload.id;
    Ok(())
}

// Core NATS with a typed queue-group handle.
#[subscribe(subject = "events.user.>", queue = ActivityWorkers)]
async fn on_user_activity_typed(message: ReceivedMessage<UserActivity>) -> NatsResult<()> {
    let _ = message.payload.kind;
    Ok(())
}

/// A payload-carrying message set.
#[derive(MessageSet)]
enum AppEvents {
    UserCreated(UserCreated),
    Order(Order),
}

#[subscribe(set)]
async fn on_event(event: AppEvents) -> NatsResult<()> {
    match event {
        AppEvents::UserCreated(user) => {
            let _ = user.user_id;
        }
        AppEvents::Order(order) => {
            let _ = order.id;
        }
    }
    Ok(())
}

#[subscribe(set, queue = ActivityWorkers)]
async fn on_event_queued(event: AppEvents) -> NatsResult<()> {
    let _ = event;
    Ok(())
}

#[test]
fn listeners_are_generated() {
    let _ = on_user_listener;
    let _ = on_user_activity_listener;
    let _ = on_order_listener;
    let _ = on_order_typed_listener;
    let _ = on_user_activity_typed_listener;
    let _ = on_event_listener;
    let _ = on_event_queued_listener;
}

#[test]
fn message_set_subjects_are_enum_namespaced() {
    assert_eq!(
        AppEvents::SUBJECTS,
        ["app.events.user.created", "app.events.order"]
    );
}

#[test]
fn describe_messages_folds_in_set_variants() {
    let subjects: Vec<_> = describe_messages().iter().map(|d| d.subject).collect();
    assert_eq!(
        subjects,
        [
            "events.user.created",
            "events.user.activity",
            "orders.created",
            "app.events.user.created",
            "app.events.order",
        ]
    );
}

#[test]
fn typed_handles_carry_their_names() {
    assert_eq!(<Orders as Stream>::NAME, "ORDERS");
    assert_eq!(<OrderProcessor as Consumer>::NAME, "order_processor");
    assert_eq!(<ActivityWorkers as Queue>::NAME, "activity_workers");
}

// Only has to type-check.
#[allow(dead_code)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
async fn listener_argument_types(client: &NatsClient, jetstream: &JetStream) {
    let _ = on_user_listener(client).await;
    let _ = on_user_activity_listener(client).await;
    let _ = on_user_activity_typed_listener(client).await;
    let _ = on_event_listener(client).await;
    let _ = on_event_queued_listener(client).await;
    let _ = on_order_listener(jetstream).await;
    let _ = on_order_typed_listener(jetstream).await;
}

#[tokio::test]
async fn a_test_calls_a_handler_with_a_delivered_message() {
    let message = ReceivedMessage::delivered(UserCreated {
        user_id: "u1".to_owned(),
    })
    .expect("the message serializes");
    assert_eq!(message.subject.as_str(), "events.user.created");
    assert_eq!(message.raw_bytes().as_ref(), br#"{"user_id":"u1"}"#);
    on_user(message).await.expect("the handler accepts it");
}

mount_workers! { on_user, on_order, self::on_event }

#[test]
fn mount_workers_describes_the_handlers_it_lists() {
    assert_eq!(
        describe_workers(),
        ["on_user", "on_order", "self::on_event"]
    );
}

// Only has to type-check.
#[allow(dead_code)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
async fn workers_start_from_one_context(context: &WorkerContext) {
    let listeners: Result<Vec<fundamental_nats::Listener>, _> = register_workers(context).await;
    let _ = listeners;
}
