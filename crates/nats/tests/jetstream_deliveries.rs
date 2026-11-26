//! What a JetStream consumer is handed: the published headers, its stream position, and a
//! termination for a payload that never decodes. Requires Docker.

use std::time::Duration;

use fundamental_nats::{
    AckPolicy, ConsumerConfig, Event, EventId, Message, NatsClient, StreamConfig,
};
use fundamental_test_containers::container_name::NATS_01_ADAPTER;
use fundamental_test_containers::nats::NatsServer;
use serde::{Deserialize, Serialize};

/// Each test publishes on its own lane, since two streams may not bind one subject.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Message)]
#[subject_fn = "lane_subject"]
struct DepositPaid {
    #[event_id]
    id: EventId,
    lane: String,
    cents: u64,
}

impl DepositPaid {
    fn on(lane: &str, cents: u64) -> Self {
        Self {
            id: EventId::generate(),
            lane: lane.to_owned(),
            cents,
        }
    }

    fn lane_subject(&self) -> String {
        subject(&self.lane)
    }
}

fn subject(lane: &str) -> String {
    format!("deliveries.{lane}")
}

async fn client() -> NatsClient {
    NatsServer::shared(NATS_01_ADAPTER)
        .await
        .expect("a NATS server")
        .client()
        .await
        .expect("connect")
}

async fn stream_with_consumer(client: &NatsClient, base: &str, consumer: &str) -> String {
    let js = client.jetstream();
    let name = NatsServer::stream_name(base).expect("a stream name");
    js.create_stream(StreamConfig::new(&name).subjects([subject(consumer)]))
        .await
        .expect("create stream");
    js.get_stream(&name)
        .await
        .expect("get stream")
        .create_consumer(
            ConsumerConfig::new(consumer)
                .durable()
                .ack_policy(AckPolicy::Explicit)
                .ack_wait(Duration::from_secs(1)),
        )
        .await
        .expect("create consumer");
    name
}

#[tokio::test]
async fn a_jetstream_delivery_keeps_its_headers() {
    let client = client().await;
    let js = client.jetstream();
    let stream = stream_with_consumer(&client, "HEADERS", "headers-worker").await;
    let event = DepositPaid::on("headers-worker", 950_000);
    js.publish_event(&event).await.expect("publish");

    let consumer = js
        .get_stream(&stream)
        .await
        .expect("stream")
        .get_consumer("headers-worker")
        .await
        .expect("consumer");
    let mut messages = consumer.messages::<DepositPaid>().await.expect("messages");
    let received = tokio::time::timeout(Duration::from_secs(5), messages.next())
        .await
        .expect("a delivery")
        .expect("the stream is open");
    assert_eq!(received.payload, event);
    let bytes = event.to_bytes().expect("bytes");
    assert_eq!(
        received.headers.get("Nats-Msg-Id"),
        Some(event.event_id().msg_id_for(&bytes).as_str()),
        "the typed path dropped the published headers"
    );
    received.ack().await.expect("ack");
}

#[tokio::test]
async fn a_raw_delivery_says_where_it_sits_and_which_attempt_it_is() {
    let client = client().await;
    let js = client.jetstream();
    let stream = stream_with_consumer(&client, "RAW_DELIVERY", "raw-worker").await;
    let event = DepositPaid::on("raw-worker", 1);
    js.publish_event(&event).await.expect("publish");

    let consumer = js
        .get_stream(&stream)
        .await
        .expect("stream")
        .get_consumer("raw-worker")
        .await
        .expect("consumer");
    let mut deliveries = consumer.deliveries().await.expect("deliveries");
    let first = tokio::time::timeout(Duration::from_secs(5), deliveries.next())
        .await
        .expect("a delivery")
        .expect("the stream is open");
    assert_eq!(first.delivery.stream_sequence, 1);
    assert_eq!(first.delivery.delivered, 1);
    assert!(
        first.names_event(event.id),
        "the delivery does not name the event it carries"
    );
    assert!(!first.names_event(EventId::generate()));
    assert_eq!(first.decode::<DepositPaid>().expect("decodes"), event);
    first.nak(Duration::ZERO).await.expect("nak");

    let second = tokio::time::timeout(Duration::from_secs(5), deliveries.next())
        .await
        .expect("a redelivery")
        .expect("the stream is open");
    assert_eq!(second.delivery.stream_sequence, 1, "the same stored copy");
    assert_eq!(second.delivery.delivered, 2, "the second attempt");
    second.ack().await.expect("ack");
}

#[tokio::test]
async fn an_undecodable_message_is_delivered_again_later_not_at_once() {
    let client = client().await;
    let js = client.jetstream();
    let stream = stream_with_consumer(&client, "UNDECODABLE", "strict-worker").await;
    js.publish_bytes(&subject("strict-worker"), "not json".into())
        .await
        .expect("publish garbage");
    let event = DepositPaid::on("strict-worker", 2);
    js.publish_event(&event).await.expect("publish");

    let mut consumer = js
        .get_stream(&stream)
        .await
        .expect("stream")
        .get_consumer("strict-worker")
        .await
        .expect("consumer");
    let mut messages = consumer.messages::<DepositPaid>().await.expect("messages");
    let received = tokio::time::timeout(Duration::from_secs(5), messages.next())
        .await
        .expect("the decodable message")
        .expect("the stream is open");
    assert_eq!(received.payload, event);
    received.ack().await.expect("ack");

    // Past the ack wait, but one sent back for later is not redelivered yet.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let info = consumer.info().await.expect("info");
    assert_eq!(
        info.num_redelivered, 0,
        "the undecodable message came straight back"
    );
}

#[tokio::test]
async fn an_event_published_twice_is_stored_once() {
    let client = client().await;
    let js = client.jetstream();
    stream_with_consumer(&client, "EVENT_DEDUP", "dedup-worker").await;
    let event = DepositPaid::on("dedup-worker", 3);
    let first = js.publish_event(&event).await.expect("publish");
    let second = js.publish_event(&event).await.expect("publish again");
    assert!(!first.duplicate);
    assert!(second.duplicate, "the broker stored the same event twice");
    assert_eq!(first.sequence, second.sequence);
}

#[tokio::test]
async fn a_consumer_reports_how_it_acknowledges_and_redelivers() {
    let client = client().await;
    let js = client.jetstream();
    let stream = stream_with_consumer(&client, "CONSUMER_INFO", "info-worker").await;
    let info = js
        .get_stream(&stream)
        .await
        .expect("stream")
        .get_consumer("info-worker")
        .await
        .expect("consumer")
        .info()
        .await
        .expect("info");
    assert_eq!(info.ack_policy, AckPolicy::Explicit);
    assert_eq!(info.ack_wait, Duration::from_secs(1));
    assert_eq!(info.max_deliver, -1, "an unset limit reads as unlimited");
    let mut handle = js.get_stream(&stream).await.expect("stream");
    let stream_info = handle.info().await.expect("stream info");
    let age = chrono::Utc::now() - stream_info.created;
    assert!(
        age < chrono::Duration::minutes(5) && age > -chrono::Duration::minutes(5),
        "the stream's creation time is not now: {}",
        stream_info.created
    );
}
