use crate::message::MessageHeaders;

pub(crate) fn extract_headers(msg: &async_nats::Message) -> MessageHeaders {
    msg.headers
        .as_ref()
        .map(MessageHeaders::from)
        .unwrap_or_default()
}
