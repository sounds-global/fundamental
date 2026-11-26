use super::MessageHeaders;
use crate::subject::Subject;

/// Options controlling a single publish: headers, a reply subject, and JetStream expectations.
#[derive(Debug, Clone, Default)]
pub struct PublishOptions {
    /// Headers to attach to the message.
    pub headers: MessageHeaders,
    /// A reply subject, for request-reply.
    pub reply: Option<Subject>,
    /// The stream the publish is expected to land in.
    #[cfg(feature = "jetstream")]
    pub expected_stream: Option<String>,
    /// The expected last message id, for idempotent publishing.
    #[cfg(feature = "jetstream")]
    pub expected_last_msg_id: Option<String>,
}

impl PublishOptions {
    /// Creates empty publish options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a header.
    #[must_use]
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key, value);
        self
    }

    /// Sets the reply subject.
    #[must_use]
    pub fn with_reply(mut self, reply: Subject) -> Self {
        self.reply = Some(reply);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_set_fields() {
        let opts = PublishOptions::new()
            .with_header("X-A", "1")
            .with_reply(Subject::new("reply.box").unwrap());
        assert_eq!(opts.headers.get("X-A"), Some("1"));
        assert_eq!(opts.reply.unwrap().as_str(), "reply.box");
    }
}
