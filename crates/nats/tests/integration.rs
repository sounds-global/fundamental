//! Live integration tests against a real NATS server.
//!
//! Requires Docker. Every subject is under this run's namespace and every stream name carries
//! its birth time, so concurrent runs never collide and a stream is genuinely new.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fundamental_nats::{
    AckPolicy, ConsumerConfig, Message, MessageSet, NatsClient, NatsError, Publish, PublishOptions,
    RpcSet, StreamConfig, Subject, SubjectPattern, SubscriptionOptions,
};
use fundamental_test_containers::container_name::NATS_01_ADAPTER;
use fundamental_test_containers::nats::NatsServer;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Message)]
#[subject = "events.user.created"]
struct UserCreated {
    user_id: String,
}

#[derive(Debug, Serialize, Deserialize, Message)]
#[subject = "rpc.echo"]
struct EchoRequest {
    text: String,
}

#[derive(Debug, Serialize, Deserialize, Message)]
#[subject = "rpc.echo.reply"]
struct EchoReply {
    text: String,
}

/// Its own subject so the header round-trip does not race the plain one.
#[derive(Serialize, Deserialize, Debug)]
struct HeaderEchoRequest {
    text: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct HeaderEchoReply {
    text: String,
}

#[allow(dead_code)] // A compile-time declaration: the derive reads the variant's types.
#[derive(RpcSet)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
enum RpcEcho {
    WithHeaders(HeaderEchoRequest, HeaderEchoReply),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Message)]
#[subject = "orders.created"]
struct OrderCreated {
    id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Message)]
#[subject = "dispatch.core.event"]
struct DispatchEvent {
    id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Message)]
#[subject = "dispatch.ok"]
struct DispatchOk {
    id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Message)]
#[subject = "dispatch.err"]
struct DispatchErr {
    id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SetCreated {
    id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SetRemoved {
    id: u64,
}

/// A payload-carrying message set. Subjects are enum-namespaced: `set.events.created` and
/// `set.events.removed`.
#[derive(Debug, Clone, PartialEq, MessageSet)]
enum SetEvents {
    Created(SetCreated),
    Removed(SetRemoved),
}

/// A client on this binary's server, reconnection off so `connect` waits for the handshake.
async fn client() -> NatsClient {
    NatsServer::shared(NATS_01_ADAPTER)
        .await
        .expect("a NATS server")
        .client()
        .await
        .expect("connect")
}

/// Publishes `message` until `sub` yields one, since a fresh subscription's interest can lag.
async fn publish_until_received<M>(
    publisher: &NatsClient,
    message: &M,
    sub: &mut fundamental_nats::Subscription<M>,
) -> fundamental_nats::ReceivedMessage<M>
where
    M: Message + Clone + Publish,
{
    for _ in 0..40 {
        publisher.publish(message.clone()).await.expect("publish");
        if let Ok(Some(received)) =
            tokio::time::timeout(Duration::from_millis(250), sub.next()).await
        {
            return received;
        }
    }
    panic!("subscription never received the message after repeated publishes");
}

#[tokio::test]
async fn publish_then_subscribe_roundtrips() {
    // Separate connections: the client defaults to `no_echo`.
    let subscriber = client().await;
    let publisher = client().await;
    let mut sub = subscriber
        .subscribe::<UserCreated>()
        .await
        .expect("subscribe");
    subscriber
        .sync()
        .await
        .expect("the subscription is registered");

    let sent = UserCreated {
        user_id: "u-1".into(),
    };
    let received = publish_until_received(&publisher, &sent, &mut sub).await;
    assert_eq!(received.payload, sent);
    assert_eq!(received.subject.as_str(), "events.user.created");
}

#[tokio::test]
async fn subscribe_dispatch_runs_handler_on_inferred_subject() {
    let subscriber = client().await;
    let publisher = client().await;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let pattern = SubjectPattern::new(DispatchEvent::SUBJECT).expect("valid pattern");
    let listener = subscriber
        .subscribe_dispatch::<DispatchEvent, _, _>(
            pattern,
            SubscriptionOptions::default(),
            move |message| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(message.payload);
                    Ok(())
                }
            },
        )
        .await
        .expect("dispatch subscribe");
    subscriber
        .sync()
        .await
        .expect("the subscription is registered");

    let received = 'received: {
        for _ in 0..40 {
            publisher
                .publish(DispatchEvent { id: 99 })
                .await
                .expect("publish");
            if let Ok(Some(payload)) =
                tokio::time::timeout(Duration::from_millis(250), rx.recv()).await
            {
                break 'received payload;
            }
        }
        panic!("dispatch handler never ran after repeated publishes");
    };
    assert_eq!(received.id, 99);
    listener.stop();
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn subscribe_set_dispatch_routes_each_variant_to_one_handler() {
    let subscriber = client().await;
    let publisher = client().await;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let listener = subscriber
        .subscribe_set_dispatch::<SetEvents, _, _>(SubscriptionOptions::default(), move |message| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(message.payload);
                Ok(())
            }
        })
        .await
        .expect("set dispatch subscribe");
    subscriber
        .sync()
        .await
        .expect("the subscription is registered");

    // Republish until both variants are observed.
    let mut saw_created = false;
    let mut saw_removed = false;
    for _ in 0..80 {
        if !saw_created {
            publisher
                .publish(SetEvents::Created(SetCreated { id: 1 }))
                .await
                .expect("publish created");
        }
        if !saw_removed {
            publisher
                .publish(SetEvents::Removed(SetRemoved { id: 2 }))
                .await
                .expect("publish removed");
        }
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(100), rx.recv()).await
        {
            match event {
                SetEvents::Created(payload) => {
                    assert_eq!(payload.id, 1);
                    saw_created = true;
                }
                SetEvents::Removed(payload) => {
                    assert_eq!(payload.id, 2);
                    saw_removed = true;
                }
            }
        }
        if saw_created && saw_removed {
            break;
        }
    }
    assert!(
        saw_created && saw_removed,
        "the single set handler must receive both variants (created: {saw_created}, removed: {saw_removed})"
    );
    listener.stop();
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn request_reply_roundtrips() {
    let requester = client().await;
    let responder = client().await;

    let mut requests = responder
        .subscribe_raw(Subject::new("rpc.echo").unwrap().into())
        .await
        .expect("subscribe responder");
    responder.sync().await.expect("the responder is registered");

    tokio::spawn(async move {
        if let Some(msg) = requests.next().await {
            let req: EchoRequest = msg.deserialize().expect("decode request");
            if let Some(reply) = msg.reply {
                // Qualified: a `Message` also carries the derived `Publish::to_bytes`.
                let bytes = Message::to_bytes(&EchoReply { text: req.text }).unwrap();
                responder
                    .publish_bytes(reply.as_str(), bytes, None)
                    .await
                    .expect("send reply");
            }
        }
    });

    // The responder's interest can lag its flush, so retry an early `NoResponders`.
    let reply: EchoReply = 'reply: {
        for _ in 0..40 {
            match requester
                .request::<EchoRequest, EchoReply>(EchoRequest {
                    text: "ping".into(),
                })
                .await
            {
                Ok(message) => break 'reply message.payload,
                Err(NatsError::NoResponders { .. }) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => panic!("request failed: {e}"),
            }
        }
        panic!("no responder ever answered");
    };
    assert_eq!(reply.text, "ping");
}

/// A request's headers reach the responder.
#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn request_headers_reach_the_responder() {
    let requester = client().await;
    let responder = client().await;

    let mut requests = responder
        .subscribe_raw(Subject::new(HeaderEchoRequest::SUBJECT).unwrap().into())
        .await
        .expect("subscribe responder");
    responder.sync().await.expect("the responder is registered");

    tokio::spawn(async move {
        while let Some(msg) = requests.next().await {
            if let Some(reply) = msg.reply {
                let seen = msg.headers.get("Authorization").unwrap_or("<absent>");
                let bytes = Message::to_bytes(&HeaderEchoReply { text: seen.into() }).unwrap();
                responder
                    .publish_bytes(reply.as_str(), bytes, None)
                    .await
                    .expect("send reply");
            }
        }
    });

    let options = PublishOptions::new().with_header("Authorization", "Bearer tok-1");
    let reply: HeaderEchoReply = 'reply: {
        for _ in 0..40 {
            match requester
                .request_with_options::<HeaderEchoRequest, HeaderEchoReply>(
                    HeaderEchoRequest {
                        text: "ping".into(),
                    },
                    options.clone(),
                )
                .await
            {
                Ok(message) => break 'reply message.payload,
                Err(NatsError::NoResponders { .. }) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => panic!("request failed: {e}"),
            }
        }
        panic!("no responder ever answered");
    };
    assert_eq!(
        reply.text, "Bearer tok-1",
        "the caller's header must reach the responder intact"
    );
}

/// Two runs sharing one server cannot see each other.
///
/// Three clients, since `no_echo` would let a two-client version pass for the wrong reason.
#[tokio::test]
async fn two_runs_on_one_server_cannot_reach_each_other() {
    let server = NatsServer::shared(NATS_01_ADAPTER)
        .await
        .expect("a NATS server");
    let in_run = async |run: &str| {
        NatsClient::connect(
            server
                .config()
                .with_subject_prefix(run)
                .expect("a valid prefix"),
        )
        .await
        .expect("connect")
    };

    let publisher = in_run("run-alpha").await;
    let same_run = in_run("run-alpha").await;
    let other_run = in_run("run-beta").await;

    // The same message type everywhere, so the subject constant is identical in all three.
    let mut heard_by_same_run = same_run
        .subscribe::<UserCreated>()
        .await
        .expect("subscribe in the publishing run");
    let mut heard_by_other_run = other_run
        .subscribe::<UserCreated>()
        .await
        .expect("subscribe in the other run");
    same_run
        .sync()
        .await
        .expect("the subscription is registered");
    other_run
        .sync()
        .await
        .expect("the subscription is registered");

    publisher
        .publish(UserCreated {
            user_id: "alpha-only".into(),
        })
        .await
        .expect("publish");
    publisher.flush().await.expect("flush the publish");

    let mine = tokio::time::timeout(Duration::from_secs(5), heard_by_same_run.next())
        .await
        .expect("a subscriber in the publishing run must hear it")
        .expect("a message");
    assert_eq!(mine.payload.user_id, "alpha-only");
    // The reported subject is the one the code spells.
    assert_eq!(mine.subject.as_str(), "events.user.created");

    assert!(
        tokio::time::timeout(Duration::from_millis(500), heard_by_other_run.next())
            .await
            .is_err(),
        "the other run heard a message published outside its namespace"
    );
}

#[tokio::test]
async fn jetstream_publish_and_consume() {
    let client = client().await;
    let js = client.jetstream();

    let orders = NatsServer::stream_name("ORDERS").expect("a stream name");
    js.create_stream(StreamConfig::new(&orders).subjects(["orders.created"]))
        .await
        .expect("create stream");

    let ack = js
        .publish(OrderCreated { id: 42 })
        .await
        .expect("js publish");
    assert_eq!(ack.stream, orders);
    assert_eq!(ack.sequence, 1);

    let stream = js.get_stream(&orders).await.expect("get stream");
    let consumer = stream
        .create_consumer(
            ConsumerConfig::new("worker")
                .durable()
                .ack_policy(AckPolicy::Explicit),
        )
        .await
        .expect("create consumer");

    let batch = consumer.fetch::<OrderCreated>(10).await.expect("fetch");
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].payload, OrderCreated { id: 42 });
    batch[0].ack().await.expect("ack");
}

#[tokio::test]
async fn jetstream_dispatch_auto_acks_on_ok() {
    let client = client().await;
    let js = client.jetstream();
    let dispatch = NatsServer::stream_name("DISPATCH_OK").expect("a stream name");
    js.create_stream(StreamConfig::new(&dispatch).subjects(["dispatch.ok"]))
        .await
        .expect("create stream");
    let stream = js.get_stream(&dispatch).await.expect("get stream");
    let mut consumer = stream
        .create_consumer(
            ConsumerConfig::new("ok-worker")
                .durable()
                .ack_policy(AckPolicy::Explicit),
        )
        .await
        .expect("create consumer");

    js.publish(DispatchOk { id: 1 }).await.expect("publish");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let listener = js
        .subscribe_consumer_dispatch::<DispatchOk, _, _>(
            &dispatch,
            "ok-worker",
            true,
            move |message| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(message.payload.id);
                    Ok(())
                }
            },
        )
        .await
        .expect("dispatch subscribe");

    let delivered = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("no timeout")
        .expect("handler delivered a message");
    assert_eq!(delivered, 1);

    // The ack travels after the handler returns, so wait for the condition, not a duration.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let cleared = loop {
        if consumer
            .info()
            .await
            .is_ok_and(|info| info.num_ack_pending == 0)
        {
            break true;
        }
        if tokio::time::Instant::now() >= deadline {
            break false;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(
        cleared,
        "an Ok handler under auto_ack must leave nothing un-acked"
    );
    listener.stop();
}

#[tokio::test]
async fn jetstream_dispatch_nacks_on_error_and_redelivers() {
    let client = client().await;
    let js = client.jetstream();
    let dispatch_err = NatsServer::stream_name("DISPATCH_ERR").expect("a stream name");
    js.create_stream(StreamConfig::new(&dispatch_err).subjects(["dispatch.err"]))
        .await
        .expect("create stream");
    let stream = js.get_stream(&dispatch_err).await.expect("get stream");
    stream
        .create_consumer(
            ConsumerConfig::new("err-worker")
                .durable()
                .ack_policy(AckPolicy::Explicit)
                .max_deliver(2)
                .ack_wait(Duration::from_secs(30)),
        )
        .await
        .expect("create consumer");

    js.publish(DispatchErr { id: 7 }).await.expect("publish");

    let attempts = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&attempts);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let listener = js
        .subscribe_consumer_dispatch::<DispatchErr, _, _>(
            &dispatch_err,
            "err-worker",
            true,
            move |_message| {
                let counter = Arc::clone(&counter);
                let tx = tx.clone();
                async move {
                    let attempt = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = tx.send(attempt);
                    Err(NatsError::internal("handler always fails"))
                }
            },
        )
        .await
        .expect("dispatch subscribe");

    let first = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("no timeout")
        .expect("first delivery");
    assert_eq!(first, 1);
    let second = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a redelivery must follow the nack")
        .expect("second delivery");
    assert_eq!(second, 2);
    listener.stop();
}

/// A TCP hop that holds every byte a client sends for `delay`, passing the server's straight back.
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn delaying_proxy(upstream: &str, delay: Duration) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    let target = upstream.trim_start_matches("nats://").to_owned();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the proxy");
    let address = listener.local_addr().expect("the proxy's address");
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let Ok(server) = TcpStream::connect(&target).await else {
                continue;
            };
            let (mut client_read, mut client_write) = client.into_split();
            let (mut server_read, mut server_write) = server.into_split();
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut server_read, &mut client_write).await;
            });
            tokio::spawn(async move {
                let mut chunk = vec![0u8; 16 * 1024];
                while let Ok(read) = client_read.read(&mut chunk).await {
                    if read == 0 {
                        break;
                    }
                    tokio::time::sleep(delay).await;
                    if server_write.write_all(&chunk[..read]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    format!("nats://{address}")
}

/// `sync` returns only after the server has registered what the connection sent, where `flush`
/// returns when the bytes leave the process; the control half proves the race is real.
#[tokio::test]
async fn sync_waits_for_the_server_to_register_a_subscription() {
    let server = NatsServer::shared(NATS_01_ADAPTER)
        .await
        .expect("a NATS server");
    let delay = Duration::from_millis(400);
    let proxied = delaying_proxy(&server.config().url, delay).await;
    let through_proxy = || {
        let mut config = server.config();
        config.url = proxied.clone();
        config
    };
    let publisher = NatsClient::connect(server.config()).await.expect("connect");

    let flushed = NatsClient::connect(through_proxy())
        .await
        .expect("connect through the proxy");
    let mut heard_after_flush = flushed.subscribe::<UserCreated>().await.expect("subscribe");
    flushed.flush().await.expect("flush");
    publisher
        .publish(UserCreated {
            user_id: "published-after-flush".into(),
        })
        .await
        .expect("publish");
    publisher.sync().await.expect("the publish is routed");
    assert!(
        tokio::time::timeout(delay * 3, heard_after_flush.next())
            .await
            .is_err(),
        "a subscription only flushed was already registered — the proxy is not delaying, so this test proves nothing"
    );
    drop(heard_after_flush);
    drop(flushed);

    let synced = NatsClient::connect(through_proxy())
        .await
        .expect("connect through the proxy");
    let mut heard_after_sync = synced.subscribe::<UserCreated>().await.expect("subscribe");
    synced.sync().await.expect("the subscription is registered");
    publisher
        .publish(UserCreated {
            user_id: "published-after-sync".into(),
        })
        .await
        .expect("publish");
    let heard = tokio::time::timeout(Duration::from_secs(5), heard_after_sync.next())
        .await
        .expect("a synced subscription hears the next publish")
        .expect("a message");
    assert_eq!(heard.payload.user_id, "published-after-sync");
}
