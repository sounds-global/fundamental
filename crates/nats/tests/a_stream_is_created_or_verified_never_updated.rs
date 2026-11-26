//! A stream is created when absent, verified when present, and never updated.
//!
//! A lookup that did not complete never reads as absent.

use std::time::Duration;

use fundamental_core::Error as _;
use fundamental_nats::{NatsClient, NatsError, StorageType, StreamConfig};
use fundamental_test_containers::container_name::NATS_01_ADAPTER;
use fundamental_test_containers::nats::{NatsAuthServer, NatsServer, NatsUser};

async fn client() -> NatsClient {
    NatsServer::shared(NATS_01_ADAPTER)
        .await
        .expect("a NATS server")
        .client()
        .await
        .expect("a client")
}

/// The subject `name`'s stream captures, one per stream.
fn subject(name: &str) -> String {
    format!("verified.{}", name.to_lowercase())
}

fn declared(name: &str) -> StreamConfig {
    StreamConfig::new(name)
        .subjects([subject(name)])
        .max_bytes(1_000_000)
        .storage(StorageType::File)
        .duplicate_window(Duration::from_secs(600))
        .deny_delete(true)
        .deny_purge(true)
}

#[tokio::test]
async fn an_absent_stream_is_created_and_reads_back_as_declared() {
    let js = client().await.jetstream();
    let name = NatsServer::stream_name("VERIFIED").expect("a stream name");

    let mut stream = js.create_stream(declared(&name)).await.expect("created");
    let info = stream.info().await.expect("its info");

    assert_eq!(info.config.name, name);
    assert_eq!(
        info.config.subjects,
        [subject(&name)],
        "as the caller named them"
    );
    assert_eq!(info.config.max_bytes, Some(1_000_000));
    assert_eq!(info.config.duplicate_window, Some(Duration::from_secs(600)));
    assert!(info.config.deny_delete && info.config.deny_purge);
    assert_eq!(info.config.max_messages, None, "unlimited reads as unset");
}

#[tokio::test]
async fn declaring_the_same_stream_again_returns_it_unchanged() {
    let js = client().await.jetstream();
    let name = NatsServer::stream_name("IDEMPOTENT").expect("a stream name");
    js.create_stream(declared(&name)).await.expect("created");

    js.create_stream(declared(&name))
        .await
        .expect("a second provisioning verifies the stream rather than failing");
    // A duplicate window left to the server is not a difference.
    js.create_stream(
        StreamConfig::new(&name)
            .subjects([subject(&name)])
            .max_bytes(1_000_000)
            .deny_delete(true)
            .deny_purge(true),
    )
    .await
    .expect("a window the declaration left unset is the server's");
}

#[tokio::test]
async fn a_changed_declaration_is_refused_naming_every_field_and_never_applied() {
    let js = client().await.jetstream();
    let name = NatsServer::stream_name("DRIFTED").expect("a stream name");
    js.create_stream(declared(&name)).await.expect("created");

    let refused = js
        .create_stream(declared(&name).max_bytes(10).deny_purge(false))
        .await
        .expect_err("a different stream under the same name is refused");

    let NatsError::StreamDrift { stream, fields } = &refused else {
        panic!("expected StreamDrift, got {refused:?}");
    };
    assert_eq!(stream, &name);
    assert!(
        fields.contains("max_bytes") && fields.contains("deny_purge"),
        "{fields}"
    );
    assert_eq!(refused.error_code(), "NATS_022");
    assert!(!refused.is_retryable());

    let mut kept = js.get_stream(&name).await.expect("the stream");
    let info = kept.info().await.expect("its info");
    assert_eq!(
        info.config.max_bytes,
        Some(1_000_000),
        "nothing was updated"
    );
    assert!(info.config.deny_purge);
}

#[tokio::test]
async fn two_callers_creating_at_once_both_end_with_the_stream() {
    let js = client().await.jetstream();
    let name = NatsServer::stream_name("RACED").expect("a stream name");
    let (left, right) = tokio::join!(
        js.create_stream(declared(&name)),
        js.create_stream(declared(&name))
    );
    left.expect("the first caller has the stream");
    right.expect("the second caller has the stream");
}

#[tokio::test]
async fn not_found_is_told_apart_from_a_lookup_that_did_not_complete() {
    let js = client().await.jetstream();
    let missing = NatsServer::stream_name("NEVER_CREATED").expect("a stream name");

    let absent = js.get_stream(&missing).await.expect_err("no such stream");
    assert!(matches!(absent, NatsError::StreamNotFound(_)), "{absent:?}");
    assert_eq!(absent.error_code(), "NATS_012");

    let malformed = js
        .get_stream("not.a.name")
        .await
        .expect_err("a malformed name");
    assert_eq!(malformed.error_code(), "NATS_011");
    assert!(
        !malformed.is_retryable(),
        "a refusal waiting will not change"
    );

    // A user the JetStream API does not answer: the lookup times out.
    let server = NatsAuthServer::shared().await.expect("the auth server");
    let unanswered = NatsClient::connect(server.config_as(NatsUser::App))
        .await
        .expect("the app user connects")
        .jetstream()
        .get_stream(&missing)
        .await
        .expect_err("the lookup does not complete");
    assert!(
        matches!(unanswered, NatsError::StreamLookupFailed { .. }),
        "{unanswered:?}"
    );
    assert_eq!(unanswered.error_code(), "NATS_020");
    assert!(unanswered.is_retryable());
}

#[tokio::test]
async fn a_missing_consumer_is_not_found_and_every_listed_stream_is_read() {
    let js = client().await.jetstream();
    let name = NatsServer::stream_name("LISTED").expect("a stream name");
    let stream = js.create_stream(declared(&name)).await.expect("created");

    let absent = stream
        .get_consumer("nobody")
        .await
        .expect_err("no such consumer");
    assert!(
        matches!(absent, NatsError::ConsumerNotFound(_)),
        "{absent:?}"
    );
    assert_eq!(absent.error_code(), "NATS_013");

    let listed = js.list_streams().await.expect("the streams");
    assert!(listed.iter().any(|info| info.name == name));
}

#[tokio::test]
async fn a_consumer_lookup_that_did_not_complete_is_not_read_as_not_found() {
    let server = NatsAuthServer::shared().await.expect("the auth server");
    let name = NatsServer::stream_name("CONSUMER_LOOKUP").expect("a stream name");
    NatsClient::connect(server.config_as(NatsUser::Provisioner))
        .await
        .expect("the provisioner connects")
        .jetstream()
        .create_stream(declared(&name))
        .await
        .expect("created");

    // A user answered about the stream, never its consumers: the consumer lookup times out.
    let stream = NatsClient::connect(server.config_as(NatsUser::Reader))
        .await
        .expect("the reader connects")
        .jetstream()
        .get_stream(&name)
        .await
        .expect("the reader may read the stream");
    let unanswered = stream
        .get_consumer("nobody")
        .await
        .expect_err("the lookup does not complete");

    assert!(
        matches!(unanswered, NatsError::ConsumerLookupFailed { .. }),
        "{unanswered:?}"
    );
    assert_eq!(unanswered.error_code(), "NATS_021");
    assert!(unanswered.is_retryable());
}
