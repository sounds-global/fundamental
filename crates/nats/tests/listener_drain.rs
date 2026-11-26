//! A responder that drains answers every request it was already handed, then leaves.
//!
//! A request delivered to a leaving queue-group member is on its connection alone, so a drop is
//! a caller's timeout. Live, because the property is what the broker already routed.

use std::sync::Arc;
use std::time::Duration;

use fundamental_macros::respond;
use fundamental_nats::{
    AckPolicy, ConsumerConfig, Message, MessageSet, NatsClient, NatsError, StreamConfig,
    SubscriptionOptions,
};
use fundamental_test_containers::container_name::NATS_01_ADAPTER;
use fundamental_test_containers::nats::NatsServer;
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, mpsc};

#[derive(Debug, Serialize, Deserialize, Message)]
#[subject = "rpc.drain.hold"]
struct Hold {
    n: u64,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Message)]
#[subject = "rpc.drain.hold.reply"]
struct Held {
    n: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Noted {
    n: u64,
}

/// A one-variant set, so the merged `#[subscribe(set)]` loop is driven.
#[derive(Debug, Clone, PartialEq, MessageSet)]
enum DrainEvents {
    Noted(Noted),
}

#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[subject = "drain.jetstream.work"]
struct Work {
    n: u64,
}

/// Holds every request inside the handler until released, so work is genuinely in flight.
struct Gate {
    entered: mpsc::UnboundedSender<u64>,
    release: Semaphore,
}

#[respond]
async fn hold(gate: &Gate, request: Hold) -> Held {
    gate.entered
        .send(request.n)
        .expect("the test holds the receiving half for its whole run");
    let _permit = gate
        .release
        .acquire()
        .await
        .expect("the gate's semaphore is never closed");
    Held { n: request.n }
}

/// How many requests are in flight when the listener is stopped.
const IN_FLIGHT: u64 = 4;

/// Long enough that only an unanswered request reaches it.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

async fn client() -> NatsClient {
    NatsServer::shared(NATS_01_ADAPTER)
        .await
        .expect("a NATS server")
        .client()
        .await
        .expect("connect")
}

#[tokio::test]
async fn a_draining_responder_answers_every_request_it_was_handed() {
    let responder = Arc::new(client().await);
    let requester = Arc::new(client().await);
    let (entered, mut arrivals) = mpsc::unbounded_channel();
    let gate = Arc::new(Gate {
        entered,
        release: Semaphore::new(0),
    });
    let listener = hold_responder(Arc::clone(&responder), Arc::clone(&gate))
        .await
        .expect("the responder subscribes");
    responder.sync().await.expect("the responder is registered");

    let requests: Vec<_> = (0..IN_FLIGHT)
        .map(|n| {
            let requester = Arc::clone(&requester);
            tokio::spawn(async move {
                requester
                    .request_with_timeout::<Hold, Held>(Hold { n }, ANSWER_WITHIN)
                    .await
                    .map(|reply| reply.payload)
            })
        })
        .collect();
    arrivals
        .recv()
        .await
        .expect("a request reaches the handler");

    // Asked while a request is held, so the drain has work in flight.
    let drained = tokio::spawn(listener.drain());
    gate.release
        .add_permits(usize::try_from(IN_FLIGHT).expect("a small count"));
    drained
        .await
        .expect("the drain task completes")
        .expect("the listener drains");

    for (n, request) in (0..IN_FLIGHT).zip(requests) {
        let answer: Result<Held, NatsError> = request.await.expect("the request task completes");
        assert_eq!(
            answer.as_ref().ok(),
            Some(&Held { n }),
            "request {n} was handed to a draining responder and never answered: {answer:?}"
        );
    }

    // The subscription is really gone, so nothing more is routed here.
    let after = requester
        .request_with_timeout::<Hold, Held>(Hold { n: IN_FLIGHT }, ANSWER_WITHIN)
        .await;
    assert!(
        matches!(after, Err(NatsError::NoResponders { .. })),
        "a drained responder still holds its subscription: {after:?}"
    );
}

/// The merged `#[subscribe(set)]` loop drains the same way.
#[tokio::test]
async fn a_draining_set_subscriber_handles_every_event_it_was_handed() {
    let subscriber = client().await;
    let publisher = client().await;
    let (entered, mut arrivals) = mpsc::unbounded_channel();
    let (handled, mut handled_events) = mpsc::unbounded_channel();
    let release = Arc::new(Semaphore::new(0));
    let gate = Arc::clone(&release);
    let listener = subscriber
        .subscribe_set_dispatch::<DrainEvents, _, _>(
            SubscriptionOptions::default(),
            move |message| {
                let (entered, handled, gate) =
                    (entered.clone(), handled.clone(), Arc::clone(&gate));
                async move {
                    let DrainEvents::Noted(noted) = message.payload;
                    entered.send(noted.n).expect("the test holds the receiver");
                    let _permit = gate.acquire().await.expect("never closed");
                    handled.send(noted.n).expect("the test holds the receiver");
                    Ok(())
                }
            },
        )
        .await
        .expect("set dispatch subscribes");
    subscriber
        .sync()
        .await
        .expect("the subscription is registered");

    for n in 0..IN_FLIGHT {
        publisher
            .publish(DrainEvents::Noted(Noted { n }))
            .await
            .expect("publish");
    }
    publisher
        .sync()
        .await
        .expect("the server routed every event");
    arrivals.recv().await.expect("an event reaches the handler");

    let drained = tokio::spawn(listener.drain());
    release.add_permits(usize::try_from(IN_FLIGHT).expect("a small count"));
    drained
        .await
        .expect("the drain task completes")
        .expect("the listener drains");

    let mut seen = Vec::new();
    while let Ok(n) = handled_events.try_recv() {
        seen.push(n);
    }
    assert_eq!(
        seen,
        (0..IN_FLIGHT).collect::<Vec<_>>(),
        "a draining set subscriber dropped events it had been handed"
    );
}

/// A JetStream loop finishes the message in hand and takes no more; the server redelivers the
/// rest.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn a_draining_jetstream_consumer_finishes_the_message_in_hand_and_leaves_the_rest() {
    let client = client().await;
    let js = client.jetstream();
    let stream_name = NatsServer::stream_name("DRAIN").expect("a stream name");
    js.create_stream(StreamConfig::new(&stream_name).subjects([Work::SUBJECT]))
        .await
        .expect("create stream");
    let stream = js.get_stream(&stream_name).await.expect("get stream");
    stream
        .create_consumer(
            ConsumerConfig::new("drain-worker")
                .durable()
                .ack_policy(AckPolicy::Explicit)
                .ack_wait(Duration::from_secs(1)),
        )
        .await
        .expect("create consumer");
    for n in 0..2 {
        js.publish(Work { n }).await.expect("publish");
    }

    let (entered, mut arrivals) = mpsc::unbounded_channel();
    let (handled, mut handled_work) = mpsc::unbounded_channel();
    let release = Arc::new(Semaphore::new(0));
    let gate = Arc::clone(&release);
    let listener = js
        .subscribe_consumer_dispatch::<Work, _, _>(
            &stream_name,
            "drain-worker",
            true,
            move |message| {
                let (entered, handled, gate) =
                    (entered.clone(), handled.clone(), Arc::clone(&gate));
                async move {
                    entered
                        .send(message.payload.n)
                        .expect("the test holds the receiver");
                    let _permit = gate.acquire().await.expect("never closed");
                    handled
                        .send(message.payload.n)
                        .expect("the test holds the receiver");
                    Ok(())
                }
            },
        )
        .await
        .expect("dispatch subscribe");
    let first = arrivals
        .recv()
        .await
        .expect("a message reaches the handler");

    let drained = tokio::spawn(listener.drain());
    release.add_permits(2);
    drained
        .await
        .expect("the drain task completes")
        .expect("the listener drains");
    assert_eq!(
        handled_work.try_recv().ok(),
        Some(first),
        "the message in hand was not finished"
    );
    assert!(
        handled_work.try_recv().is_err(),
        "a draining JetStream loop took another message after its drain was asked for"
    );

    let (redelivered, mut redeliveries) = mpsc::unbounded_channel();
    let successor = js
        .subscribe_consumer_dispatch::<Work, _, _>(
            &stream_name,
            "drain-worker",
            true,
            move |message| {
                let redelivered = redelivered.clone();
                async move {
                    redelivered
                        .send(message.payload.n)
                        .expect("the test holds the receiver");
                    Ok(())
                }
            },
        )
        .await
        .expect("the successor subscribes");
    let left = tokio::time::timeout(ANSWER_WITHIN * 2, redeliveries.recv())
        .await
        .expect("the message left behind is redelivered")
        .expect("the successor's handler ran");
    assert_eq!(
        left,
        1 - first,
        "the successor received the message the drain left"
    );
    successor.drain().await.expect("the successor drains");
}
