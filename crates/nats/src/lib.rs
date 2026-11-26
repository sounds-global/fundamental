//! Type-safe NATS messaging for the Fundamental framework.
//!
//! The framework's seam over [`async-nats`](async_nats): typed publish/subscribe,
//! request-reply and JetStream, each message declaring its subject through [`Message`]:
//!
//! ```
//! # extern crate fundamental_nats as fundamental;
//! use fundamental::Message;
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize, Message)]
//! #[subject = "events.user.created"]
//! struct UserCreated {
//!     user_id: String,
//! }
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

extern crate self as fundamental_nats;

mod client;
mod config;
#[cfg(feature = "jetstream")]
mod consumer;
mod error;
mod jetstream;
mod message;
mod message_descriptor;
mod queue;
#[cfg(feature = "jetstream")]
mod stream;
mod subject;
mod subscription;
pub mod tracing;
mod worker;
mod worker_context;
mod worker_guard;

pub use client::{ConnectionEvent, ConnectionState, NatsClient, NatsProbe};
pub use config::{
    NatsClientAuth, NatsConfig, NatsTlsConfig, NkeyTransitSigner, ReconnectConfig,
    nkey_user_public_key,
};
#[cfg(feature = "jetstream")]
pub use consumer::Consumer;
pub use error::{NatsError, NatsResult};
pub use fundamental_macros::{Message, MessageSet, RpcSet};
// Named by `#[derive(MessageSet)]`-generated code.
pub use bytes::Bytes;
// For generated code: the default JSON codec a derived `MessageSet` encodes with.
#[doc(no_inline)]
pub use fundamental_macros::Queue;
#[cfg(feature = "jetstream")]
#[doc(no_inline)]
pub use fundamental_macros::{Consumer, Stream};
pub use fundamental_secrets::StoreAnswer;
#[cfg(feature = "jetstream")]
pub use jetstream::{
    AckPolicy, ConsumerConfig, ConsumerInfo, DeliverPolicy, Delivery, DiscardPolicy, JetStream,
    JetStreamConfig, JetStreamConsumer, JetStreamDeliveries, JetStreamDelivery, JetStreamStream,
    JetStreamSubscription, PublishAck, RawMessageStream, ReplayPolicy, RetentionPolicy,
    StorageType, StreamConfig, StreamInfo,
};
pub use message::{
    DeadLetter, DeadLetterReason, Event, EventId, EventPayload, Message, MessageEnvelope,
    MessageHeaders, MessageSet, PayloadDigest, Publish, PublishOptions, ReceivedMessage,
    RespondMeta, RpcSet,
};
pub use message_descriptor::{MessageDescriptor, MessageTransport};
pub use queue::Queue;
#[doc(hidden)]
pub use serde_json;
#[cfg(feature = "jetstream")]
pub use stream::Stream;

// So `mount_messages!`-generated code reaches the schema surface through this crate alone.
#[doc(no_inline)]
pub use fundamental_schema::{SchemaNode, describe};
pub use subject::{Subject, SubjectBuilder, SubjectPattern, SubjectPrefix};
pub use subscription::{
    DeadLetterConfig, DeclaredSubscription, DrainRequested, Listener, RawReceivedMessage,
    RawSubscription, Subscription, SubscriptionOptions,
};
pub use worker::Worker;
pub use worker_context::{ListenerFuture, WorkerContext, WorkersFuture};
pub use worker_guard::WorkerGuard;
