/// A subscription the server refused, parsed from its `-ERR` text; publish violations are not
/// modelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionsViolation {
    /// The subject the server refused.
    pub subject: String,
    /// The queue group named in the refusal, if the subscription used one.
    pub queue_group: Option<String>,
}

/// Recover the refused subject from a NATS `-ERR` message.
///
/// `async_nats` gives only the server's text, so this is the one place that parses it; a format
/// change returns `None` rather than a wrong subject.
pub(crate) fn parse_permissions_violation(text: &str) -> Option<PermissionsViolation> {
    const SUBSCRIPTION_MARKER: &str = "Permissions Violation for Subscription to ";
    const QUEUE_MARKER: &str = " using queue ";

    let rest = text.split_once(SUBSCRIPTION_MARKER)?.1;
    let (subject, after) = quoted(rest)?;
    let queue_group = after
        .split_once(QUEUE_MARKER)
        .and_then(|(_, tail)| quoted(tail))
        .map(|(group, _)| group);

    Some(PermissionsViolation {
        subject,
        queue_group,
    })
}

/// The first double-quoted run in `text`, and whatever follows it.
///
/// A subject cannot contain a quote or whitespace, so the naive scan is exact.
fn quoted(text: &str) -> Option<(String, &str)> {
    let opened = text.strip_prefix('"').or_else(|| {
        let at = text.find('"')?;
        text.get(at + 1..)
    })?;
    let close = opened.find('"')?;
    Some((opened[..close].to_string(), &opened[close + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_subscription_refusal_names_its_subject() {
        let violation = parse_permissions_violation(
            r#"Permissions Violation for Subscription to "events.layer1.credential.changed""#,
        )
        .expect("parses");
        assert_eq!(violation.subject, "events.layer1.credential.changed");
        assert_eq!(violation.queue_group, None);
    }

    #[test]
    fn a_queue_subscription_refusal_names_both() {
        let violation = parse_permissions_violation(
            r#"Permissions Violation for Subscription to "rpc.layer1.erase" using queue "layer1_erase""#,
        )
        .expect("parses");
        assert_eq!(violation.subject, "rpc.layer1.erase");
        assert_eq!(violation.queue_group.as_deref(), Some("layer1_erase"));
    }

    /// A publish violation is not mistaken for a subscription one.
    #[test]
    fn a_publish_violation_is_not_a_subscription_violation() {
        assert_eq!(
            parse_permissions_violation(r#"Permissions Violation for Publish to "acme.audit.rpc""#),
            None
        );
    }

    #[test]
    fn an_unrelated_server_error_is_ignored() {
        for text in [
            "Authorization Violation",
            "Maximum Payload Violation",
            "Slow Consumer",
            "",
        ] {
            assert_eq!(parse_permissions_violation(text), None, "{text}");
        }
    }

    /// A truncated or reformatted message degrades to `None`.
    #[test]
    fn a_malformed_message_yields_nothing() {
        for text in [
            "Permissions Violation for Subscription to",
            r#"Permissions Violation for Subscription to "unterminated"#,
        ] {
            assert_eq!(parse_permissions_violation(text), None, "{text}");
        }
    }
}
