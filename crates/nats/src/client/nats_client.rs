use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use async_nats::Client;
use bytes::Bytes;
use fundamental_core::Error as _;
use fundamental_core::retry::OsEntropy;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{Instrument, debug, error, info, info_span, instrument, warn};

use super::wire::Wire;
use super::{ConnectionEvent, ConnectionState};
use crate::config::{NatsClientAuth, NatsConfig, step_for_attempt};
use crate::error::{NatsError, NatsResult};
use crate::message::{
    Message, MessageEnvelope, MessageHeaders, MessageSet, Publish, PublishOptions, ReceivedMessage,
};
use crate::subject::{Subject, SubjectPattern};
use crate::subscription::headers::extract_headers;
use crate::subscription::permissions_violation::parse_permissions_violation;
use crate::subscription::subscription_inventory::SubscriptionInventory;
use crate::subscription::{
    DeclaredSubscription, Listener, RawSubscription, Subscription, SubscriptionOptions,
};
use crate::tracing::DeliveryIds;

#[cfg(feature = "jetstream")]
use crate::jetstream::JetStream;

/// Refuse an inbox prefix a client could not safely draw replies under.
///
/// `async_nats` subscribes replies under `{prefix}.{nuid}.*`, so a wildcard prefix would
/// receive every reply in the system; checked at `connect`, the one fallible step.
///
/// # Errors
/// [`NatsError::Config`] naming the setting.
fn validate_inbox_prefix(prefix: &str) -> NatsResult<()> {
    let refuse = |why: &str| {
        Err(NatsError::Config(format!(
            "inbox_prefix {prefix:?} is unusable: {why}"
        )))
    };
    if prefix.is_empty() {
        return refuse("it is empty");
    }
    if prefix.contains('>') || prefix.contains('*') {
        return refuse(
            "it contains a wildcard, which would subscribe this client to every reply \
             under that subject tree instead of its own",
        );
    }
    if prefix.chars().any(char::is_whitespace) {
        return refuse("it contains whitespace, which is not valid in a NATS subject");
    }
    if prefix.starts_with('.') || prefix.ends_with('.') || prefix.contains("..") {
        return refuse("it has an empty subject token");
    }
    Ok(())
}

/// A type-safe NATS client: publish and subscribe to [`Message`] types, perform
/// request-reply, and observe connection state.
#[derive(Debug)]
pub struct NatsClient {
    /// The connection, and the namespace every subject it names is relative to.
    ///
    /// A [`Wire`], so no subject reaches the server without the prefix.
    inner: Wire,
    config: NatsConfig,
    state: Arc<RwLock<ConnectionState>>,
    draining: AtomicBool,
    messages_published: AtomicU64,
    messages_received: AtomicU64,
    event_tx: broadcast::Sender<ConnectionEvent>,
    /// What this process declared it subscribes to, and which the server refused.
    subscriptions: Arc<SubscriptionInventory>,
}

impl NatsClient {
    /// Connects to NATS using `config`.
    ///
    /// # Errors
    /// Returns [`NatsError::Connection`] if the connection cannot be established, or
    /// [`NatsError::Config`] if the inbox prefix or the reconnect schedule is unusable.
    #[instrument(skip(config), fields(url = %config.url), err(Debug))]
    pub async fn connect(config: NatsConfig) -> NatsResult<Self> {
        validate_inbox_prefix(&config.inbox_prefix)?;
        info!(url = %config.url, "connecting to NATS");

        let (event_tx, _) = broadcast::channel(16);
        let state = Arc::new(RwLock::new(ConnectionState::Connecting));
        let subscriptions = Arc::new(SubscriptionInventory::default());

        let options = build_options(
            &config,
            state.clone(),
            event_tx.clone(),
            subscriptions.clone(),
        )?;
        let client = options.connect(&config.url).await?;
        *state.write() = ConnectionState::Connected;

        info!(
            url = %config.url,
            server_id = %client.server_info().server_id,
            "connected to NATS"
        );

        Ok(Self {
            inner: Wire::new(client, config.subject_prefix.clone()),
            config,
            state,
            draining: AtomicBool::new(false),
            messages_published: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            event_tx,
            subscriptions,
        })
    }

    /// Returns the current connection state.
    #[must_use]
    pub fn state(&self) -> ConnectionState {
        *self.state.read()
    }

    /// Returns whether the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.state() == ConnectionState::Connected
    }

    /// Returns the server information reported by the connection.
    #[must_use]
    pub fn server_info(&self) -> async_nats::ServerInfo {
        self.inner.raw().server_info()
    }

    /// Returns the number of messages published by this client.
    #[must_use]
    pub fn messages_published(&self) -> u64 {
        self.messages_published.load(Ordering::Relaxed)
    }

    /// Returns the number of messages received by this client.
    #[must_use]
    pub fn messages_received(&self) -> u64 {
        self.messages_received.load(Ordering::Relaxed)
    }

    /// Drops the current connection and connects again, authenticating afresh.
    ///
    /// How a credential rotated in the secret store reaches a live connection.
    ///
    /// # Errors
    /// [`NatsError::Connection`] when the client has closed and can no longer reconnect.
    #[instrument(skip(self), err(Debug))]
    pub async fn force_reconnect(&self) -> NatsResult<()> {
        self.inner.raw().force_reconnect().await.map_err(|error| {
            NatsError::connection(format!(
                "the client could not be asked to reconnect: {error}"
            ))
        })?;
        info!("reconnect requested");
        Ok(())
    }

    /// Subscribes to connection-state-change events.
    #[must_use]
    pub fn subscribe_events(&self) -> broadcast::Receiver<ConnectionEvent> {
        self.event_tx.subscribe()
    }

    /// Publishes a value to its own subject.
    ///
    /// A [`MessageSet`](crate::MessageSet) variant sends to its active variant's subject.
    ///
    /// # Errors
    /// Returns an error if serialization fails or the payload exceeds the limit.
    #[instrument(skip(self, message), fields(subject = %message.subject()), err(Debug))]
    pub async fn publish<P: Publish>(&self, message: P) -> NatsResult<()> {
        let subject = message.subject();
        let payload = message.to_bytes()?;
        self.publish_bytes(&subject, payload, None).await
    }

    /// Publishes a value with explicit [`PublishOptions`].
    ///
    /// # Errors
    /// Returns an error if serialization fails or the payload exceeds the limit.
    #[instrument(skip(self, message, options), fields(subject = %message.subject()), err(Debug))]
    pub async fn publish_with_options<P: Publish>(
        &self,
        message: P,
        options: PublishOptions,
    ) -> NatsResult<()> {
        let subject = message.subject();
        let payload = message.to_bytes()?;
        self.publish_bytes(&subject, payload, Some(options)).await
    }

    /// Publishes a message to a specific `subject`.
    ///
    /// # Errors
    /// Returns an error if serialization fails or the payload exceeds the limit.
    #[instrument(skip(self, message), fields(subject = %subject), err(Debug))]
    pub async fn publish_to<M: Message>(&self, subject: &Subject, message: M) -> NatsResult<()> {
        let payload = message.to_bytes()?;
        self.publish_bytes(subject.as_str(), payload, None).await
    }

    /// Publishes a message wrapped in a [`MessageEnvelope`].
    ///
    /// # Errors
    /// Returns an error if serialization fails or the payload exceeds the limit.
    #[instrument(skip(self, envelope), fields(subject = %envelope.payload.subject()), err(Debug))]
    pub async fn publish_envelope<M: Message>(
        &self,
        envelope: MessageEnvelope<M>,
    ) -> NatsResult<()> {
        let subject = envelope.payload.subject();
        let payload = envelope.to_bytes()?;
        self.publish_bytes(&subject, payload, None).await
    }

    /// Publishes raw `payload` bytes to `subject`.
    ///
    /// # Errors
    /// Returns [`NatsError::MessageTooLarge`] if the payload exceeds the limit, or
    /// a publish error from the server.
    #[instrument(skip(self, payload, options), fields(subject, bytes = payload.len()), err(Debug))]
    pub async fn publish_bytes(
        &self,
        subject: &str,
        payload: Bytes,
        options: Option<PublishOptions>,
    ) -> NatsResult<()> {
        self.check_payload_size(payload.len())?;
        let subject = subject.to_string();
        let mut options = options.unwrap_or_default();

        // Every publish funnels here, so this carries the trace across the hop.
        fundamental_observability::trace_context::inject_current(&mut options.headers);

        match (options.reply, options.headers.is_empty()) {
            (Some(reply), true) => {
                self.inner
                    .publish_with_reply(subject.as_str(), reply.into_inner(), payload)
                    .await?;
            }
            (Some(reply), false) => {
                self.inner
                    .publish_with_reply_and_headers(
                        subject.as_str(),
                        reply.into_inner(),
                        options.headers.into(),
                        payload,
                    )
                    .await?;
            }
            (None, false) => {
                self.inner
                    .publish_with_headers(subject.as_str(), options.headers.into(), payload)
                    .await?;
            }
            (None, true) => {
                self.inner.publish(subject.as_str(), payload).await?;
            }
        }

        self.messages_published.fetch_add(1, Ordering::Relaxed);
        debug!("published message");
        Ok(())
    }

    /// Subscribes to a message type on its own subject.
    ///
    /// # Errors
    /// Returns an error if the subject is invalid or the subscribe fails.
    pub async fn subscribe<M: Message>(&self) -> NatsResult<Subscription<M>> {
        let pattern = SubjectPattern::new(M::SUBJECT)?;
        self.subscribe_pattern(pattern, SubscriptionOptions::default())
            .await
    }

    /// Subscribes to a message type with explicit [`SubscriptionOptions`].
    ///
    /// # Errors
    /// Returns an error if the subject is invalid or the subscribe fails.
    pub async fn subscribe_with_options<M: Message>(
        &self,
        options: SubscriptionOptions,
    ) -> NatsResult<Subscription<M>> {
        let pattern = SubjectPattern::new(M::SUBJECT)?;
        self.subscribe_pattern(pattern, options).await
    }

    /// Subscribes to a `pattern`, distributing across a queue group if set.
    ///
    /// # Errors
    /// Returns [`NatsError::Subscription`] if the subscribe fails.
    #[instrument(skip(self, options), fields(pattern = %pattern, queue_group), err(Debug))]
    pub async fn subscribe_pattern<M: Message>(
        &self,
        pattern: SubjectPattern,
        options: SubscriptionOptions,
    ) -> NatsResult<Subscription<M>> {
        let subscriber = if let Some(group) = &options.queue_group {
            self.inner
                .queue_subscribe(pattern.as_str(), group.clone())
                .await?
        } else {
            self.inner.subscribe(pattern.as_str()).await?
        };
        debug!(pattern = %pattern, "created subscription");
        Ok(Subscription::new(
            subscriber,
            pattern,
            options,
            self.inner.prefix().cloned(),
        ))
    }

    /// Subscribes to a `pattern` yielding raw, undeserialized messages.
    ///
    /// # Errors
    /// Returns [`NatsError::Subscription`] if the subscribe fails.
    #[instrument(skip(self), fields(pattern = %pattern), err(Debug))]
    pub async fn subscribe_raw(&self, pattern: SubjectPattern) -> NatsResult<RawSubscription> {
        let subscriber = self.inner.subscribe(pattern.as_str()).await?;
        debug!(pattern = %pattern, "created raw subscription");
        Ok(RawSubscription::new(
            subscriber,
            pattern,
            self.inner.prefix().cloned(),
        ))
    }

    /// Subscribes to `pattern` and spawns a background loop that runs `handler`
    /// for every message, returning a [`Listener`] guard over that loop.
    ///
    /// Each message runs in its own span with the propagated trace context; a handler error is
    /// logged once here, so the handler must not log it too.
    ///
    /// # Errors
    /// Returns [`NatsError::Subscription`] if the subscribe fails.
    pub async fn subscribe_dispatch<M, H, Fut>(
        &self,
        pattern: SubjectPattern,
        options: SubscriptionOptions,
        handler: H,
    ) -> NatsResult<Listener>
    where
        M: Message,
        H: Fn(ReceivedMessage<M>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = NatsResult<()>> + Send + 'static,
    {
        let declared = DeclaredSubscription {
            subject: pattern.as_str().to_string(),
            queue_group: options.queue_group.clone(),
        };
        let mut subscription = self.subscribe_pattern::<M>(pattern, options).await?;
        self.subscriptions.declare(declared.clone());
        let pattern = subscription.pattern().to_string();
        Ok(Listener::spawn_tracked(
            Arc::clone(&self.subscriptions),
            vec![declared],
            move |mut drain| async move {
                info!(pattern, "dispatch loop started");
                loop {
                    // `biased`: a drain is noticed only between deliveries.
                    tokio::select! {
                        biased;
                        () = drain.arrived() => {
                            match subscription.drain().await {
                                Ok(()) => {
                                    info!(pattern, "draining: unsubscribed, answering what was already delivered");
                                    while let Some(message) = subscription.next().await {
                                        dispatch_one(&handler, message).await;
                                    }
                                }
                                Err(error) => {
                                    warn!(pattern, %error, "draining: the unsubscribe could not be sent, so nothing more can arrive; ending");
                                }
                            }
                            break;
                        }
                        message = subscription.next() => match message {
                            Some(message) => dispatch_one(&handler, message).await,
                            None => break,
                        },
                    }
                }
                info!(pattern, "dispatch loop ended; subscription closed");
            },
        ))
    }

    /// Subscribes to every subject in a [`MessageSet`] and routes each delivery to its variant in
    /// one loop, the runtime behind `#[subscribe(set)]`.
    ///
    /// The returned [`Listener`] owns every subscription; an undecodable delivery is logged and
    /// skipped.
    ///
    /// # Errors
    /// Returns [`NatsError::Subscription`] if any of the per-subject subscribes fails.
    pub async fn subscribe_set_dispatch<S, H, Fut>(
        &self,
        options: SubscriptionOptions,
        handler: H,
    ) -> NatsResult<Listener>
    where
        S: MessageSet,
        H: Fn(ReceivedMessage<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = NatsResult<()>> + Send + 'static,
    {
        use futures::stream::{StreamExt, select_all};

        let mut subscribers = Vec::with_capacity(S::SUBJECTS.len());
        let mut declared = Vec::with_capacity(S::SUBJECTS.len());
        for subject in S::SUBJECTS {
            declared.push(DeclaredSubscription {
                subject: (*subject).to_string(),
                queue_group: options.queue_group.clone(),
            });
            let subscriber = if let Some(group) = &options.queue_group {
                self.inner.queue_subscribe(subject, group.clone()).await?
            } else {
                self.inner.subscribe(subject).await?
            };
            subscribers.push(subscriber);
        }
        debug!(subjects = ?S::SUBJECTS, "created message-set subscriptions");
        for entry in &declared {
            self.subscriptions.declare(entry.clone());
        }

        let mut merged = select_all(subscribers);
        let subjects = S::SUBJECTS;
        // The namespace travels with the deliveries into the loop.
        let wire = self.inner.clone();
        Ok(Listener::spawn_tracked(
            Arc::clone(&self.subscriptions),
            declared,
            move |mut drain| async move {
                info!(?subjects, "set dispatch loop started");
                loop {
                    // `biased`, as in `subscribe_dispatch`.
                    let raw = tokio::select! {
                        biased;
                        () = drain.arrived() => {
                            let mut unsubscribed = true;
                            for subscriber in merged.iter_mut() {
                                if let Err(error) = subscriber.drain().await {
                                    unsubscribed = false;
                                    warn!(?subjects, %error, "draining: an unsubscribe could not be sent");
                                }
                            }
                            if unsubscribed {
                                info!(?subjects, "draining: unsubscribed, answering what was already delivered");
                                while let Some(raw) = merged.next().await {
                                    dispatch_one_set::<S, _, _>(&handler, wire.strip_delivery(raw)).await;
                                }
                            }
                            break;
                        }
                        raw = merged.next() => match raw {
                            Some(raw) => raw,
                            None => break,
                        },
                    };
                    // Stripped, since `S::from_parts` routes on the unprefixed subject.
                    dispatch_one_set::<S, _, _>(&handler, wire.strip_delivery(raw)).await;
                }
                info!(?subjects, "set dispatch loop ended; subscriptions closed");
            },
        ))
    }

    /// The subscription the server refused, if any — what readiness reports.
    ///
    /// Only the `subscribe_dispatch` family is tracked; `subscribe_pattern` and `subscribe_raw`
    /// return a stream whose lifetime the client cannot observe.
    #[must_use]
    pub fn refused_subscription(&self) -> Option<DeclaredSubscription> {
        self.subscriptions.refused()
    }

    /// Every subscription this client declared, with the queue group it asked to
    /// join, in declaration order.
    ///
    /// The read-back a service checks its declared delivery shape against. Tracked as in
    /// [`refused_subscription`](Self::refused_subscription).
    #[must_use]
    pub fn declared_subscriptions(&self) -> Vec<DeclaredSubscription> {
        self.subscriptions.declared()
    }

    /// Sends a request to its subject and awaits a typed reply within the configured timeout.
    ///
    /// # Errors
    /// Returns [`NatsError::Timeout`], [`NatsError::NoResponders`], or a
    /// serialization error.
    pub async fn request<Req: Message, Res: Message>(
        &self,
        request: Req,
    ) -> NatsResult<ReceivedMessage<Res>> {
        self.request_with_timeout(request, self.config.request_timeout)
            .await
    }

    /// Sends a request with an explicit `timeout`.
    ///
    /// # Errors
    /// Returns [`NatsError::Timeout`], [`NatsError::NoResponders`], or a
    /// serialization error.
    #[instrument(skip(self, request), fields(subject = %request.subject(), timeout_ms = timeout.as_millis() as u64), err(Debug))]
    pub async fn request_with_timeout<Req: Message, Res: Message>(
        &self,
        request: Req,
        timeout: Duration,
    ) -> NatsResult<ReceivedMessage<Res>> {
        let subject = request.subject().to_string();
        let payload = request.to_bytes()?;
        self.check_payload_size(payload.len())?;
        self.request_inner(subject, payload, MessageHeaders::new(), timeout)
            .await
    }

    /// Sends a request to a specific `subject`.
    ///
    /// # Errors
    /// Returns [`NatsError::Timeout`], [`NatsError::NoResponders`], or a
    /// serialization error.
    #[instrument(skip(self, request), fields(subject = %subject), err(Debug))]
    pub async fn request_to<Req: Message, Res: Message>(
        &self,
        subject: &Subject,
        request: Req,
    ) -> NatsResult<ReceivedMessage<Res>> {
        let payload = request.to_bytes()?;
        self.check_payload_size(payload.len())?;
        self.request_inner(
            subject.as_str().to_string(),
            payload,
            MessageHeaders::new(),
            self.config.request_timeout,
        )
        .await
    }

    /// Sends a request carrying the headers in `options`.
    ///
    /// Headers carry per-request metadata such as a credential, which must never ride in the
    /// payload; the responder reads them from [`RespondMeta`](crate::message::RespondMeta).
    /// `options.reply` is ignored.
    ///
    /// # Errors
    /// Returns [`NatsError::Timeout`], [`NatsError::NoResponders`], or a
    /// serialization error.
    #[instrument(skip(self, request, options), fields(subject = %request.subject()), err(Debug))]
    pub async fn request_with_options<Req: Message, Res: Message>(
        &self,
        request: Req,
        options: PublishOptions,
    ) -> NatsResult<ReceivedMessage<Res>> {
        let subject = request.subject().to_string();
        let payload = request.to_bytes()?;
        self.check_payload_size(payload.len())?;
        self.request_inner(
            subject,
            payload,
            options.headers,
            self.config.request_timeout,
        )
        .await
    }

    async fn request_inner<Res: Message>(
        &self,
        subject: String,
        payload: Bytes,
        mut headers: MessageHeaders,
        timeout: Duration,
    ) -> NatsResult<ReceivedMessage<Res>> {
        // Injected over the caller's headers, so a caller cannot forge a `traceparent`.
        fundamental_observability::trace_context::inject_current(&mut headers);

        let sent = async {
            if headers.is_empty() {
                self.inner.request(subject.as_str(), payload).await
            } else {
                self.inner
                    .request_with_headers(subject.as_str(), headers.into(), payload)
                    .await
            }
        };

        let response = tokio::time::timeout(timeout, sent)
            .await
            .map_err(|_| NatsError::Timeout {
                timeout_ms: timeout.as_millis() as u64,
            })?
            .map_err(|e| match e.kind() {
                async_nats::client::RequestErrorKind::TimedOut => NatsError::Timeout {
                    timeout_ms: timeout.as_millis() as u64,
                },
                async_nats::client::RequestErrorKind::NoResponders => {
                    NatsError::NoResponders { subject }
                }
                _ => NatsError::publish(e.to_string()),
            })?;

        self.messages_received.fetch_add(1, Ordering::Relaxed);
        let res_subject = Subject::new_unchecked(response.subject.as_str());
        let headers = extract_headers(&response);
        let raw = Bytes::copy_from_slice(&response.payload);
        let payload = Res::from_bytes(&response.payload)?;
        Ok(ReceivedMessage::new(
            payload,
            res_subject,
            None,
            headers,
            raw,
        ))
    }

    /// Returns a JetStream context for durable messaging.
    #[cfg(feature = "jetstream")]
    #[must_use]
    pub fn jetstream(&self) -> JetStream {
        JetStream::new(self.inner.clone())
    }

    /// Flushes buffered messages to the server.
    ///
    /// # Errors
    /// Returns [`NatsError::Internal`] if the flush fails.
    #[instrument(skip(self), err(Debug))]
    pub async fn flush(&self) -> NatsResult<()> {
        self.inner
            .raw()
            .flush()
            .await
            .map_err(|e| NatsError::internal(e.to_string()))
    }

    /// Returns once the server has processed every command this connection sent before the
    /// call.
    ///
    /// Unlike [`flush`](Self::flush), which returns when the bytes leave: it asks the server for
    /// a reply on an unsubscribed inbox, whose *no responders* answer comes in order. Needs
    /// publish permission on this client's inbox prefix.
    ///
    /// # Errors
    /// [`NatsError::Timeout`] when the server does not answer within the request timeout, or
    /// [`NatsError::Publish`] when the request cannot be sent.
    #[instrument(skip(self), err(Debug))]
    pub async fn sync(&self) -> NatsResult<()> {
        let timeout = self.config.request_timeout;
        let barrier = self.inner.raw().new_inbox();
        match tokio::time::timeout(timeout, self.inner.raw().request(barrier, Bytes::new())).await {
            Err(_) => Err(NatsError::Timeout {
                timeout_ms: timeout.as_millis() as u64,
            }),
            Ok(Err(error))
                if error.kind() == async_nats::client::RequestErrorKind::NoResponders =>
            {
                Ok(())
            }
            Ok(Err(error)) if error.kind() == async_nats::client::RequestErrorKind::TimedOut => {
                Err(NatsError::Timeout {
                    timeout_ms: timeout.as_millis() as u64,
                })
            }
            Ok(Err(error)) => Err(NatsError::publish(error.to_string())),
            Ok(Ok(reply)) => {
                warn!(subject = %reply.subject, "a sync barrier inbox was answered; treating the round trip as complete");
                Ok(())
            }
        }
    }

    /// Drains the connection: stops new messages and processes pending ones.
    ///
    /// # Errors
    /// Returns [`NatsError::Internal`] if the drain fails.
    #[instrument(skip(self), err(Debug))]
    pub async fn drain(&self) -> NatsResult<()> {
        self.draining.store(true, Ordering::SeqCst);
        self.inner
            .raw()
            .drain()
            .await
            .map_err(|e| NatsError::internal(e.to_string()))
    }

    /// Returns the underlying `async-nats` client for operations not covered here.
    #[must_use]
    pub fn raw_client(&self) -> &Client {
        self.inner.raw()
    }

    fn check_payload_size(&self, size: usize) -> NatsResult<()> {
        if size > self.config.max_payload {
            return Err(NatsError::MessageTooLarge {
                size,
                max_size: self.config.max_payload,
            });
        }
        Ok(())
    }
}

/// Connect options that authenticate through a callback answered at each connection attempt.
///
/// A stored credential answers from the value last read; an NKey has its signer sign the nonce,
/// so the seed never enters the process.
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
fn auth_callback(auth: &NatsClientAuth) -> Option<async_nats::ConnectOptions> {
    match auth {
        NatsClientAuth::StoredUserPassword { username, password } => {
            let (username, password) = (username.clone(), password.clone());
            Some(async_nats::ConnectOptions::with_auth_callback(
                move |_nonce| {
                    let mut answer = async_nats::Auth::new();
                    answer.username = Some(username.clone());
                    answer.password = Some(password.current().expose().clone());
                    debug!(secret.source = %password.reference(), age_seconds = password.age().as_secs(), "answering NATS authentication from the stored password");
                    async move { Ok(answer) }
                },
            ))
        }
        NatsClientAuth::StoredToken { token } => {
            let token = token.clone();
            Some(async_nats::ConnectOptions::with_auth_callback(
                move |_nonce| {
                    let mut answer = async_nats::Auth::new();
                    answer.token = Some(token.current().expose().clone());
                    debug!(secret.source = %token.reference(), age_seconds = token.age().as_secs(), "answering NATS authentication from the stored token");
                    async move { Ok(answer) }
                },
            ))
        }
        NatsClientAuth::NKey(signer) => {
            let signer = signer.clone();
            Some(async_nats::ConnectOptions::with_auth_callback(
                move |nonce| {
                    // Its own task: the callback's future must be `Sync`.
                    let signer = signer.clone();
                    let signing = tokio::spawn(async move {
                        let signed = signer.sign(&nonce).await;
                        (signer, signed)
                    });
                    async move {
                        match signing.await {
                            Ok((signer, Ok(signature))) => {
                                let mut answer = async_nats::Auth::new();
                                answer.nkey = Some(signer.public_key().to_string());
                                answer.signature = Some(signature.to_vec());
                                Ok(answer)
                            }
                            Ok((signer, Err(failure))) => {
                                error!(
                                    error_code = failure.error_code(),
                                    error = %failure,
                                    nkey.signer = %signer.signer(),
                                    retryable = failure.is_retryable(),
                                    "the NKey nonce was not signed; this connection attempt fails"
                                );
                                Err(async_nats::AuthError::new(failure.error_code()))
                            }
                            Err(joined) => {
                                error!(error = %joined, "the NKey signing task ended without an answer");
                                Err(async_nats::AuthError::new("NATS_025"))
                            }
                        }
                    }
                },
            ))
        }
        NatsClientAuth::None
        | NatsClientAuth::UserPassword { .. }
        | NatsClientAuth::Token { .. }
        | NatsClientAuth::CredentialsFile { .. } => None,
    }
}

/// Whether the client may add the servers a server advertises to its reconnect pool.
///
/// Not when it presents a credential without TLS, since a peer would receive it in the clear.
fn follows_discovered_servers(config: &NatsConfig) -> bool {
    config.tls.is_some() || matches!(config.auth, NatsClientAuth::None)
}

#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
fn build_options(
    config: &NatsConfig,
    state: Arc<RwLock<ConnectionState>>,
    event_tx: broadcast::Sender<ConnectionEvent>,
    subscriptions: Arc<SubscriptionInventory>,
) -> NatsResult<async_nats::ConnectOptions> {
    let mut options = match auth_callback(&config.auth) {
        Some(answering) => answering,
        None => async_nats::ConnectOptions::new(),
    };

    // rustls panics when two providers are compiled and none is installed; installing
    // aws-lc-rs keeps such an app working. Unconditional, since a `tls://` URL negotiates TLS
    // anyway, and idempotent.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    if let Some(name) = &config.name {
        options = options.name(name.clone());
    }

    match &config.auth {
        NatsClientAuth::None
        | NatsClientAuth::StoredUserPassword { .. }
        | NatsClientAuth::StoredToken { .. }
        | NatsClientAuth::NKey(_) => {}
        NatsClientAuth::UserPassword { username, password } => {
            options = options.user_and_password(username.clone(), password.expose().clone());
        }
        NatsClientAuth::Token { token } => {
            options = options.token(token.expose().clone());
        }
        // Fail closed: an unreadable credentials file never falls back to anonymous.
        NatsClientAuth::CredentialsFile { path } => {
            // Read here, synchronously: an `async` read would overflow callers' `recursion_limit`.
            let creds = std::fs::read_to_string(path).map_err(|error| {
                NatsError::Authentication(format!(
                    "the credentials file {} could not be read: {error}",
                    path.display()
                ))
            })?;
            options = options.credentials(&creds).map_err(|error| {
                NatsError::Authentication(format!(
                    "the credentials file {} is not a valid NATS credential: {error}",
                    path.display()
                ))
            })?;
        }
    }

    if !follows_discovered_servers(config) {
        options = options.ignore_discovered_servers();
    }

    if let Some(tls) = &config.tls {
        if tls.required {
            options = options.require_tls(true);
        }
        if let Some(ca) = &tls.ca_cert {
            options = options.add_root_certificates(ca.clone());
        }
        if let (Some(cert), Some(key)) = (&tls.client_cert, &tls.client_key) {
            options = options.add_client_certificate(cert.clone(), key.clone());
        }
    }

    options = options
        .connection_timeout(config.connect_timeout)
        .request_timeout(Some(config.request_timeout))
        .ping_interval(config.ping_interval)
        .custom_inbox_prefix(config.inbox_prefix.clone());

    // Full jitter and no reconnect budget: a spent `max_reconnects` closes the client for good.
    if config.reconnect.retry_initial_connect {
        options = options.retry_on_initial_connect();
    }
    let backoff = config.reconnect.backoff()?;
    options = options
        .reconnect_delay_callback(move |attempt| {
            let delay = backoff.draw(step_for_attempt(attempt), None, &mut OsEntropy);
            debug!(
                attempt,
                delay_ms = delay.as_millis(),
                "nats reconnect backoff"
            );
            delay
        })
        .max_reconnects(None)
        .subscription_capacity(config.subscription_capacity);
    if !config.echo {
        options = options.no_echo();
    }

    Ok(options.event_callback(move |event| {
        let state = state.clone();
        let tx = event_tx.clone();
        let subscriptions = Arc::clone(&subscriptions);
        async move {
            // A subscription refusal arrives only here, since `subscribe` is fire-and-forget.
            if let async_nats::Event::ServerError(server_error) = &event
                && let Some(violation) = parse_permissions_violation(&server_error.to_string())
            {
                subscriptions.refuse(&violation);
            }
            let (transition, error) = classify_event(event);
            let now = {
                let mut current = state.write();
                if let Some(transition) = transition {
                    *current = transition;
                }
                *current
            };
            let _ = tx.send(ConnectionEvent { state: now, error });
        }
    }))
}

/// The state `event` moves the connection to, if any, and the error it reports.
///
/// Only a connect and a disconnect move it.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn classify_event(event: async_nats::Event) -> (Option<ConnectionState>, Option<String>) {
    match event {
        async_nats::Event::Connected => {
            info!("NATS connected");
            (Some(ConnectionState::Connected), None)
        }
        async_nats::Event::Disconnected => {
            info!("NATS disconnected");
            (Some(ConnectionState::Disconnected), None)
        }
        async_nats::Event::ServerError(err) => {
            error!(error = %err, "NATS server error");
            (None, Some(err.to_string()))
        }
        async_nats::Event::ClientError(err) => {
            error!(error = %err, "NATS client error");
            (None, Some(err.to_string()))
        }
        async_nats::Event::SlowConsumer(sid) => {
            warn!(subscription = sid, "NATS slow consumer");
            (None, Some(format!("slow consumer on sid {sid}")))
        }
        other => {
            debug!(event = ?other, "NATS connection event");
            (None, None)
        }
    }
}

/// Runs `handler` for one received message inside a per-message span carrying the
/// subject and any propagated trace context, logging the outcome once.
async fn dispatch_one<M, H, Fut>(handler: &H, message: ReceivedMessage<M>)
where
    M: Message,
    H: Fn(ReceivedMessage<M>) -> Fut,
    Fut: Future<Output = NatsResult<()>>,
{
    // The publisher's ids are recorded only as a `RequestId`.
    let ids = DeliveryIds::from_headers(&message.headers);
    let span = info_span!(
        "nats.message",
        subject = %message.subject,
        request_id = ids.request_id(),
    );
    // Before `.instrument`: a span's parent is fixed when it is entered.
    fundamental_observability::trace_context::adopt_parent(&span, &message.headers);
    async {
        ids.log();
        match handler(message).await {
            Ok(()) => debug!("handler completed"),
            Err(error) => {
                error!(error = %error, error.code = error.error_code(), "handler failed");
            }
        }
    }
    .instrument(span)
    .await;
}

/// Routes one raw delivery to its [`MessageSet`] variant and runs `handler` in a span; an
/// undecodable payload is logged and skipped.
async fn dispatch_one_set<S, H, Fut>(handler: &H, raw: async_nats::Message)
where
    S: MessageSet,
    H: Fn(ReceivedMessage<S>) -> Fut,
    Fut: Future<Output = NatsResult<()>>,
{
    let subject = Subject::new_unchecked(raw.subject.as_str());
    let reply = raw
        .reply
        .as_ref()
        .map(|s| Subject::new_unchecked(s.as_str()));
    let headers = extract_headers(&raw);
    let bytes = Bytes::copy_from_slice(&raw.payload);
    let ids = DeliveryIds::from_headers(&headers);
    let span = info_span!(
        "nats.message",
        subject = %subject,
        request_id = ids.request_id(),
    );
    fundamental_observability::trace_context::adopt_parent(&span, &headers);
    async {
        ids.log();
        match S::from_parts(raw.subject.as_str(), &raw.payload) {
            Ok(payload) => {
                let message = ReceivedMessage::new(payload, subject, reply, headers, bytes);
                match handler(message).await {
                    Ok(()) => debug!("handler completed"),
                    Err(error) => {
                        error!(error = %error, error.code = error.error_code(), "handler failed");
                    }
                }
            }
            Err(error) => {
                warn!(error = %error, error.code = error.error_code(), "skipping undecodable set message");
            }
        }
    }
    .instrument(span)
    .await;
}

impl Clone for NatsClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: self.config.clone(),
            state: self.state.clone(),
            draining: AtomicBool::new(self.draining.load(Ordering::SeqCst)),
            messages_published: AtomicU64::new(self.messages_published.load(Ordering::Relaxed)),
            messages_received: AtomicU64::new(self.messages_received.load(Ordering::Relaxed)),
            event_tx: self.event_tx.clone(),
            subscriptions: Arc::clone(&self.subscriptions),
        }
    }
}

#[cfg(test)]
mod tests {
    /// A wildcard prefix would subscribe this client to every reply under it.
    #[test]
    fn a_wildcard_inbox_prefix_is_refused() {
        for prefix in ["_INBOX.>", "_INBOX.*", ">", "a.*.b"] {
            let error = super::validate_inbox_prefix(prefix)
                .expect_err("a wildcard prefix must never reach connect");
            assert!(
                error.to_string().contains("wildcard"),
                "{prefix}: the refusal should name the reason — {error}"
            );
        }
    }

    #[test]
    fn a_malformed_inbox_prefix_is_refused() {
        for prefix in ["", "_INBOX.", ".x", "a..b", "has space"] {
            assert!(
                super::validate_inbox_prefix(prefix).is_err(),
                "{prefix:?} would produce a malformed subject the server rejects \
                 per-message rather than at connect"
            );
        }
    }

    #[test]
    fn an_ordinary_inbox_prefix_is_accepted() {
        for prefix in ["_INBOX", "_INBOX.orders", "_INBOX_orders", "replies.svc-1"] {
            assert!(
                super::validate_inbox_prefix(prefix).is_ok(),
                "{prefix:?} is a valid subject and should be usable"
            );
        }
    }

    use super::*;

    use tracing_test::traced_test;

    use crate::config::ReconnectConfig;

    #[derive(serde::Serialize, serde::Deserialize, fundamental_macros::Message)]
    #[subject = "test.dispatch"]
    struct Tick {
        n: u64,
    }

    fn tick(n: u64) -> ReceivedMessage<Tick> {
        ReceivedMessage::new(
            Tick { n },
            Subject::new("test.dispatch").expect("valid subject"),
            None,
            MessageHeaders::new(),
            Bytes::new(),
        )
    }

    /// The dispatch boundary invokes the handler for each message.
    #[tokio::test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn dispatch_one_runs_the_handler() {
        let total = Arc::new(AtomicU64::new(0));
        let seen = Arc::clone(&total);
        let handler = move |message: ReceivedMessage<Tick>| {
            let seen = Arc::clone(&seen);
            async move {
                seen.fetch_add(message.payload.n, Ordering::SeqCst);
                Ok(())
            }
        };
        dispatch_one(&handler, tick(2)).await;
        dispatch_one(&handler, tick(3)).await;
        assert_eq!(total.load(Ordering::SeqCst), 5);
    }

    /// A handler error is logged once, with its stable code, and does not end the loop.
    #[traced_test]
    #[tokio::test]
    async fn dispatch_one_logs_handler_error_with_code() {
        let handler =
            |_message: ReceivedMessage<Tick>| async { Err(NatsError::internal("downstream down")) };
        dispatch_one(&handler, tick(1)).await;
        assert!(logs_contain("handler failed"), "the failure must be logged");
        assert!(logs_contain("NATS_016"), "the stable code must be logged");
    }

    /// A delivery span records the publisher's ids only as UUIDs.
    #[traced_test]
    #[tokio::test]
    async fn a_delivery_span_records_only_uuid_ids() {
        const REQUEST: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";
        let mut headers = MessageHeaders::new();
        headers.insert(crate::tracing::HEADER_REQUEST_ID, REQUEST);
        headers.insert(crate::tracing::HEADER_USER_ID, "jan@example.nl");
        headers.insert(
            crate::tracing::HEADER_TENANT_ID,
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        );
        headers.insert(crate::tracing::HEADER_TRACE_ID, "jan-trace@example.nl");
        let message = ReceivedMessage::new(
            Tick { n: 1 },
            Subject::new("test.dispatch").expect("valid subject"),
            None,
            headers,
            Bytes::new(),
        );
        let handler = |_message: ReceivedMessage<Tick>| async { Ok(()) };
        dispatch_one(&handler, message).await;
        assert!(logs_contain("handler completed"), "the span saw an event");
        assert!(logs_contain(REQUEST), "a UUID request id is recorded");
        assert!(!logs_contain("jan"), "a non-UUID user or trace id is not");
        assert!(
            !logs_contain("9f86d081884c7d65"),
            "nor a token-shaped tenant id"
        );
    }

    fn options_for(config: &NatsConfig) -> NatsResult<async_nats::ConnectOptions> {
        let (event_tx, _) = broadcast::channel(16);
        build_options(
            config,
            Arc::new(RwLock::new(ConnectionState::Connecting)),
            event_tx,
            Arc::new(SubscriptionInventory::default()),
        )
    }

    #[test]
    fn nats_never_gives_up_on_its_own() {
        for reconnect in [ReconnectConfig::default(), ReconnectConfig::fail_fast()] {
            let options = options_for(&NatsConfig::default().with_reconnect(reconnect))
                .unwrap_or_else(|error| panic!("{error}"));
            let rendered = format!("{options:?}");
            assert!(
                rendered.contains("\"max_reconnects\": None"),
                "a spent max_reconnects closes an async-nats client for good: {rendered}"
            );
        }
    }

    #[test]
    fn a_zero_width_reconnect_schedule_is_refused_at_connect() {
        let config = NatsConfig::default()
            .with_reconnect(ReconnectConfig::default().with_initial_delay(Duration::ZERO));
        match options_for(&config) {
            Err(error @ NatsError::Config(_)) => assert_eq!(error.error_code(), "NATS_015"),
            other => panic!("a zero first window was accepted: {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn config_defaults_hold() {
        let config = NatsConfig::default();
        assert_eq!(config.url, "nats://localhost:4222");
        assert!(config.reconnect.retry_initial_connect);
    }

    #[test]
    fn a_plaintext_secret_never_follows_an_advertised_server() {
        let plain = NatsConfig::new("nats://127.0.0.1:4222");
        assert!(
            follows_discovered_servers(&plain),
            "no secret, nothing to carry"
        );
        assert!(!follows_discovered_servers(
            &plain.clone().with_credentials("app", "pw")
        ));
        assert!(!follows_discovered_servers(
            &plain.clone().with_token("tok")
        ));
        assert!(!follows_discovered_servers(
            &plain.clone().with_credentials_file("/run/app.creds")
        ));
        assert!(follows_discovered_servers(
            &plain
                .with_credentials("app", "pw")
                .with_tls(crate::NatsTlsConfig::new())
        ));
    }

    #[test]
    fn classify_known_events() {
        let (state, err) = classify_event(async_nats::Event::Connected);
        assert_eq!(state, Some(ConnectionState::Connected));
        assert!(err.is_none());

        let (state, err) = classify_event(async_nats::Event::Disconnected);
        assert_eq!(state, Some(ConnectionState::Disconnected));
        assert!(err.is_none());
    }

    #[test]
    fn an_error_reports_itself_without_claiming_a_connection() {
        let (state, err) = classify_event(async_nats::Event::ClientError(
            async_nats::ClientError::Other("authorization violation".to_owned()),
        ));
        assert_eq!(
            state, None,
            "a failed reconnect attempt is not a connection"
        );
        assert!(
            err.is_some_and(|reported| reported.contains("authorization violation")),
            "the error is still reported"
        );
        let (state, _) = classify_event(async_nats::Event::SlowConsumer(7));
        assert_eq!(state, None);
    }
}
