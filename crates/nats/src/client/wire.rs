use async_nats::{Client, HeaderMap, Message, SubscribeError, Subscriber};
use bytes::Bytes;

use crate::subject::SubjectPrefix;

/// The connection, plus the namespace every subject it names is relative to.
///
/// The raw handle is private and every operation resolves its subject here, so no path can
/// skip the [`SubjectPrefix`]. [`NatsClient::raw_client`](super::NatsClient::raw_client) is the
/// documented exception.
#[derive(Debug, Clone)]
pub(crate) struct Wire {
    client: Client,
    prefix: Option<SubjectPrefix>,
}

impl Wire {
    /// Wraps `client`, naming every subject under `prefix`.
    pub(crate) fn new(client: Client, prefix: Option<SubjectPrefix>) -> Self {
        Self { client, prefix }
    }

    /// The underlying client, for the paths documented as taking the wire as it is.
    pub(crate) fn raw(&self) -> &Client {
        &self.client
    }

    /// This connection's namespace, for a JetStream stream's `subjects`.
    pub(crate) fn prefix(&self) -> Option<&SubjectPrefix> {
        self.prefix.as_ref()
    }

    /// `subject`, as it goes on the wire.
    pub(crate) fn resolve(&self, subject: &str) -> String {
        resolve_with(self.prefix.as_ref(), subject)
    }

    /// `subject`, as the code above this connection spells it.
    ///
    /// A subject without the prefix, such as an `_INBOX` reply, comes back untouched.
    pub(crate) fn strip<'a>(&self, subject: &'a str) -> &'a str {
        strip_with(self.prefix.as_ref(), subject)
    }

    /// The same, for a delivery: rewrites the message's subject in place.
    pub(crate) fn strip_delivery(&self, mut message: Message) -> Message {
        let stripped = self.strip(message.subject.as_str());
        if stripped.len() != message.subject.len() {
            message.subject = stripped.to_owned().into();
        }
        message
    }

    pub(crate) async fn publish(
        &self,
        subject: &str,
        payload: Bytes,
    ) -> Result<(), async_nats::PublishError> {
        self.client.publish(self.resolve(subject), payload).await
    }

    pub(crate) async fn publish_with_headers(
        &self,
        subject: &str,
        headers: HeaderMap,
        payload: Bytes,
    ) -> Result<(), async_nats::PublishError> {
        self.client
            .publish_with_headers(self.resolve(subject), headers, payload)
            .await
    }

    pub(crate) async fn publish_with_reply(
        &self,
        subject: &str,
        reply: String,
        payload: Bytes,
    ) -> Result<(), async_nats::PublishError> {
        self.client
            .publish_with_reply(self.resolve(subject), reply, payload)
            .await
    }

    pub(crate) async fn publish_with_reply_and_headers(
        &self,
        subject: &str,
        reply: String,
        headers: HeaderMap,
        payload: Bytes,
    ) -> Result<(), async_nats::PublishError> {
        self.client
            .publish_with_reply_and_headers(self.resolve(subject), reply, headers, payload)
            .await
    }

    pub(crate) async fn subscribe(&self, pattern: &str) -> Result<Subscriber, SubscribeError> {
        self.client.subscribe(self.resolve(pattern)).await
    }

    pub(crate) async fn queue_subscribe(
        &self,
        pattern: &str,
        group: String,
    ) -> Result<Subscriber, SubscribeError> {
        self.client
            .queue_subscribe(self.resolve(pattern), group)
            .await
    }

    pub(crate) async fn request(
        &self,
        subject: &str,
        payload: Bytes,
    ) -> Result<Message, async_nats::RequestError> {
        self.client.request(self.resolve(subject), payload).await
    }

    pub(crate) async fn request_with_headers(
        &self,
        subject: &str,
        headers: HeaderMap,
        payload: Bytes,
    ) -> Result<Message, async_nats::RequestError> {
        self.client
            .request_with_headers(self.resolve(subject), headers, payload)
            .await
    }
}

/// NATS's reserved namespace for reply addresses.
///
/// `NatsConfig::inbox_prefix` narrows it per service rather than replacing it.
const INBOX_ROOT: &str = "_INBOX";

/// Whether `subject` is an address this connection was **handed** rather than one it named.
///
/// A reply address was minted by the requester, so re-namespacing it would send the answer
/// nowhere. The root, not this connection's prefix, since the address is the requester's.
fn is_reply_address(subject: &str) -> bool {
    subject == INBOX_ROOT || subject.starts_with(concat!("_INBOX", "."))
}

/// `subject` under `prefix`, or unchanged when there is none.
fn resolve_with(prefix: Option<&SubjectPrefix>, subject: &str) -> String {
    match prefix {
        Some(prefix) if !is_reply_address(subject) => prefix.apply(subject),
        _ => subject.to_owned(),
    }
}

/// `subject` with `prefix` removed, or unchanged when there is none.
fn strip_with<'a>(prefix: Option<&SubjectPrefix>, subject: &'a str) -> &'a str {
    prefix.map_or(subject, |prefix| prefix.strip(subject))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(prefix: &str) -> SubjectPrefix {
        SubjectPrefix::new(prefix).expect("a valid prefix")
    }

    #[test]
    fn a_wire_with_no_prefix_changes_nothing() {
        // `None` is byte-identical in both directions.
        assert_eq!(resolve_with(None, "orders.created"), "orders.created");
        assert_eq!(strip_with(None, "orders.created"), "orders.created");
    }

    #[test]
    fn a_prefixed_wire_names_and_unnames_symmetrically() {
        let run = prefix("run-7f3a");
        assert_eq!(
            resolve_with(Some(&run), "orders.created"),
            "run-7f3a.orders.created"
        );
        assert_eq!(
            strip_with(Some(&run), "run-7f3a.orders.created"),
            "orders.created"
        );
        assert_eq!(
            strip_with(Some(&run), resolve_with(Some(&run), "a.b").as_str()),
            "a.b"
        );
    }

    #[test]
    fn a_reply_inbox_is_not_this_namespaces_business() {
        // Replies are a separate namespace; trimming would corrupt the reply subject.
        let run = prefix("run-7f3a");
        assert_eq!(strip_with(Some(&run), "_INBOX.abc.1"), "_INBOX.abc.1");
    }

    #[test]
    fn a_reply_is_sent_to_the_address_it_was_given() {
        // A re-namespaced reply publishes fine to nobody, and the requester times out.
        let run = prefix("run-7f3a");
        assert_eq!(
            resolve_with(Some(&run), "_INBOX.abc.1"),
            "_INBOX.abc.1",
            "a reply address is minted by the requester and already names a live subscription"
        );
        // Another service's inbox is still absolute.
        assert_eq!(
            resolve_with(Some(&run), "_INBOX.layer1.7f3a"),
            "_INBOX.layer1.7f3a"
        );
        // A subject that only looks like one is not: the boundary is a token.
        assert_eq!(
            resolve_with(Some(&run), "_INBOXES.orders"),
            "run-7f3a._INBOXES.orders"
        );
    }
}
