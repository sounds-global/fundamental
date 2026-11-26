//! `#[subscribe]`: an async handler on a NATS subject, a JetStream consumer or a WebSocket action.

use proc_macro::TokenStream;
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{ItemFn, LitStr, parse_macro_input};

use crate::attr::name_arg::NameArg;
use crate::attr::{
    subscribe_inbox, subscribe_jetstream, subscribe_nats, subscribe_nats_set, subscribe_socket,
};

pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut args = SubscribeArgs::default();
    let parser = syn::meta::parser(|meta| args.parse(meta));
    parse_macro_input!(attr with parser);
    let item = parse_macro_input!(item as ItemFn);

    match build(args, item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// The raw `#[subscribe(...)]` keys; spans are kept so an invalid use points at its key.
#[derive(Default)]
struct SubscribeArgs {
    subject: Option<LitStr>,
    queue: Option<NameArg>,
    stream: Option<NameArg>,
    consumer: Option<NameArg>,
    auto_ack: Option<Span>,
    // Bare `set`: one handler for every subject of a `MessageSet`.
    set: Option<Span>,
    // `Some(None)` is bare `socket` (message mode, gated); `Some(Some(action))` is `socket = "action"` (raw, ungated).
    socket: Option<Option<LitStr>>,
    // `inbox = "effect"`: the effect commits with an inbox row, once per event.
    inbox: Option<LitStr>,
    // `without_inbox = "reason"`: why a JetStream handler is idempotent without the inbox.
    without_inbox: Option<LitStr>,
    // `isolation = Serializable`: the inbox transaction's level.
    isolation: Option<syn::Ident>,
}

impl SubscribeArgs {
    fn parse(&mut self, meta: syn::meta::ParseNestedMeta) -> syn::Result<()> {
        if meta.path.is_ident("subject") {
            self.subject = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("queue") {
            self.queue = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("stream") {
            self.stream = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("consumer") {
            self.consumer = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("auto_ack") {
            self.auto_ack = Some(meta.path.span());
        } else if meta.path.is_ident("set") {
            self.set = Some(meta.path.span());
        } else if meta.path.is_ident("socket") {
            // A missing `=` is the bare form, not an error.
            self.socket = Some(match meta.value() {
                Ok(value) => Some(value.parse()?),
                Err(_) => None,
            });
        } else if meta.path.is_ident("inbox") {
            self.inbox = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("without_inbox") {
            self.without_inbox = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("isolation") {
            self.isolation = Some(meta.value()?.parse()?);
        } else {
            return Err(meta.error(
                "unknown `#[subscribe(...)]` key; valid keys: subject, queue, stream, consumer, auto_ack, set, socket, inbox, without_inbox, isolation",
            ));
        }
        Ok(())
    }
}

/// The only legal shapes of `#[subscribe]`.
#[derive(Debug)]
enum Transport {
    NatsCore {
        subject: Option<LitStr>,
        queue: Option<NameArg>,
    },
    JetStream {
        stream: NameArg,
        consumer: NameArg,
        auto_ack: bool,
    },
    JetStreamInbox {
        stream: NameArg,
        consumer: NameArg,
        effect: LitStr,
        isolation: syn::Ident,
    },
    /// A core-NATS `MessageSet`; `queue` load-balances the whole set.
    NatsSet {
        queue: Option<NameArg>,
    },
    Socket {
        action: Option<LitStr>,
    },
}

fn build(args: SubscribeArgs, item: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    if item.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            item.sig.fn_token,
            "a `#[subscribe]` handler must be an `async fn`",
        ));
    }
    match classify(args)? {
        Transport::NatsCore { subject, queue } => subscribe_nats::expand(item, subject, queue),
        Transport::JetStream {
            stream,
            consumer,
            auto_ack,
        } => subscribe_jetstream::expand(item, stream, consumer, auto_ack),
        Transport::JetStreamInbox {
            stream,
            consumer,
            effect,
            isolation,
        } => subscribe_inbox::expand(item, stream, consumer, &effect, &isolation),
        Transport::NatsSet { queue } => subscribe_nats_set::expand(item, queue),
        Transport::Socket { action } => subscribe_socket::expand(item, action),
    }
}

/// Classifies the keys into one transport, refusing every illegal combination at its key.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn classify(args: SubscribeArgs) -> syn::Result<Transport> {
    let SubscribeArgs {
        subject,
        queue,
        stream,
        consumer,
        auto_ack,
        set,
        socket,
        inbox,
        without_inbox,
        isolation,
    } = args;

    let jetstream = stream.is_some() || consumer.is_some();
    if !jetstream {
        let other = if set.is_some() {
            "set"
        } else if socket.is_some() {
            "socket"
        } else {
            "core NATS"
        };
        if let Some(token) = &inbox {
            return Err(at(token.span(), "inbox", other));
        }
        if let Some(token) = &without_inbox {
            return Err(at(token.span(), "without_inbox", other));
        }
        if let Some(token) = &isolation {
            return Err(at(token.span(), "isolation", other));
        }
    }

    if let Some(set_span) = set {
        // A set's subjects come from the enum over core NATS; only `queue` may join it.
        if let Some(token) = &subject {
            return Err(at(token.span(), "subject", "set"));
        }
        for (token, key) in [(&stream, "stream"), (&consumer, "consumer")] {
            if let Some(token) = token {
                return Err(at(token.span(), key, "set"));
            }
        }
        if let Some(span) = auto_ack {
            return Err(at(span, "auto_ack", "set"));
        }
        if socket.is_some() {
            return Err(at(set_span, "set", "socket"));
        }
        return Ok(Transport::NatsSet { queue });
    }

    if let Some(action) = socket {
        if let Some(token) = &subject {
            return Err(at(token.span(), "subject", "socket"));
        }
        if let Some(token) = &queue {
            return Err(at(token.span(), "queue", "socket"));
        }
        for (token, key) in [(&stream, "stream"), (&consumer, "consumer")] {
            if let Some(token) = token {
                return Err(at(token.span(), key, "socket"));
            }
        }
        if let Some(span) = auto_ack {
            return Err(at(span, "auto_ack", "socket"));
        }
        return Ok(Transport::Socket { action });
    }

    match (stream, consumer) {
        (Some(stream), Some(consumer)) => {
            if let Some(token) = &subject {
                return Err(at(token.span(), "subject", "stream/consumer"));
            }
            if let Some(token) = &queue {
                return Err(at(token.span(), "queue", "stream/consumer"));
            }
            match (inbox, without_inbox) {
                (Some(effect), None) => {
                    if let Some(span) = auto_ack {
                        return Err(syn::Error::new(
                            span,
                            "`auto_ack` cannot be combined with `inbox`: the inbox settles each delivery once its effect has committed or failed",
                        ));
                    }
                    let Some(isolation) = isolation else {
                        return Err(syn::Error::new(
                            effect.span(),
                            "an inbox `#[subscribe]` must say the level its transaction runs at: `isolation = Serializable`, `RepeatableRead` or `ReadCommitted`",
                        ));
                    };
                    if !ISOLATION_LEVELS.contains(&isolation.to_string().as_str()) {
                        return Err(syn::Error::new(
                            isolation.span(),
                            "`isolation` is `Serializable`, `RepeatableRead` or `ReadCommitted`",
                        ));
                    }
                    Ok(Transport::JetStreamInbox {
                        stream,
                        consumer,
                        effect,
                        isolation,
                    })
                }
                (None, Some(reason)) => {
                    if let Some(token) = &isolation {
                        return Err(at(token.span(), "isolation", "without_inbox"));
                    }
                    if reason.value().trim().is_empty() {
                        return Err(syn::Error::new(
                            reason.span(),
                            "`without_inbox` names why a second delivery has no second effect",
                        ));
                    }
                    Ok(Transport::JetStream {
                        stream,
                        consumer,
                        auto_ack: auto_ack.is_some(),
                    })
                }
                (Some(effect), Some(_)) => Err(at(effect.span(), "inbox", "without_inbox")),
                (None, None) => Err(syn::Error::new(
                    stream.span(),
                    "a JetStream `#[subscribe]` is delivered at least once, so it must choose: `inbox = \"<effect>\"` for one effect per event, or `without_inbox = \"<why a redelivery is harmless>\"`",
                )),
            }
        }
        (Some(stream), None) => Err(syn::Error::new(
            stream.span(),
            "JetStream `#[subscribe]` requires both `stream` and `consumer`",
        )),
        (None, Some(consumer)) => Err(syn::Error::new(
            consumer.span(),
            "JetStream `#[subscribe]` requires both `stream` and `consumer`",
        )),
        (None, None) => {
            if let Some(span) = auto_ack {
                return Err(syn::Error::new(
                    span,
                    "`auto_ack` only applies to a JetStream `#[subscribe]` (with `stream` and `consumer`)",
                ));
            }
            Ok(Transport::NatsCore { subject, queue })
        }
    }
}

/// The isolation levels an inbox transaction may run at; `ReadUncommitted` is not one.
const ISOLATION_LEVELS: [&str; 3] = ["Serializable", "RepeatableRead", "ReadCommitted"];

fn at(span: Span, key: &str, other: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!("`{key}` cannot be combined with `{other}` in `#[subscribe]`"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(value: &str) -> LitStr {
        LitStr::new(value, Span::call_site())
    }

    fn name(value: &str) -> NameArg {
        NameArg::Name(lit(value))
    }

    #[test]
    fn bare_is_nats_core_with_inferred_subject() {
        let transport = classify(SubscribeArgs::default()).expect("bare is legal");
        assert!(matches!(
            transport,
            Transport::NatsCore {
                subject: None,
                queue: None
            }
        ));
    }

    #[test]
    fn explicit_subject_and_queue_is_nats_core() {
        let args = SubscribeArgs {
            subject: Some(lit("events.user.>")),
            queue: Some(name("workers")),
            ..Default::default()
        };
        assert!(matches!(
            classify(args).expect("legal"),
            Transport::NatsCore {
                subject: Some(_),
                queue: Some(_)
            }
        ));
    }

    #[test]
    fn bare_set_is_nats_set() {
        let args = SubscribeArgs {
            set: Some(Span::call_site()),
            ..Default::default()
        };
        assert!(matches!(
            classify(args).expect("bare set is legal"),
            Transport::NatsSet { queue: None }
        ));
    }

    #[test]
    fn set_with_queue_is_nats_set() {
        let args = SubscribeArgs {
            set: Some(Span::call_site()),
            queue: Some(name("workers")),
            ..Default::default()
        };
        assert!(matches!(
            classify(args).expect("set + queue is legal"),
            Transport::NatsSet { queue: Some(_) }
        ));
    }

    #[test]
    fn set_rejects_subject() {
        let args = SubscribeArgs {
            set: Some(Span::call_site()),
            subject: Some(lit("events.user.>")),
            ..Default::default()
        };
        let err = classify(args).expect_err("set + subject is illegal");
        assert!(err.to_string().contains("subject"));
        assert!(err.to_string().contains("set"));
    }

    #[test]
    fn set_rejects_jetstream_keys() {
        let args = SubscribeArgs {
            set: Some(Span::call_site()),
            stream: Some(name("ORDERS")),
            ..Default::default()
        };
        let err = classify(args).expect_err("set + stream is illegal");
        assert!(err.to_string().contains("stream"));
        assert!(err.to_string().contains("set"));
    }

    #[test]
    fn set_rejects_auto_ack() {
        let args = SubscribeArgs {
            set: Some(Span::call_site()),
            auto_ack: Some(Span::call_site()),
            ..Default::default()
        };
        let err = classify(args).expect_err("set + auto_ack is illegal");
        assert!(err.to_string().contains("auto_ack"));
    }

    #[test]
    fn set_rejects_socket() {
        let args = SubscribeArgs {
            set: Some(Span::call_site()),
            socket: Some(None),
            ..Default::default()
        };
        let err = classify(args).expect_err("set + socket is illegal");
        assert!(err.to_string().contains("set"));
        assert!(err.to_string().contains("socket"));
    }

    #[test]
    fn stream_and_consumer_with_auto_ack_is_jetstream() {
        let args = SubscribeArgs {
            stream: Some(name("ORDERS")),
            consumer: Some(name("proc")),
            auto_ack: Some(Span::call_site()),
            without_inbox: Some(lit("the order id is the booking's key")),
            ..Default::default()
        };
        assert!(matches!(
            classify(args).expect("legal"),
            Transport::JetStream { auto_ack: true, .. }
        ));
    }

    #[test]
    fn explicit_socket_action_is_raw_mode() {
        let args = SubscribeArgs {
            socket: Some(Some(lit("chat"))),
            ..Default::default()
        };
        assert!(matches!(
            classify(args).expect("legal"),
            Transport::Socket { action: Some(_) }
        ));
    }

    #[test]
    fn bare_socket_is_message_mode() {
        let args = SubscribeArgs {
            socket: Some(None),
            ..Default::default()
        };
        assert!(matches!(
            classify(args).expect("legal"),
            Transport::Socket { action: None }
        ));
    }

    #[test]
    fn socket_rejects_jetstream_keys() {
        let args = SubscribeArgs {
            socket: Some(Some(lit("chat"))),
            stream: Some(name("ORDERS")),
            ..Default::default()
        };
        let err = classify(args).expect_err("socket + stream is illegal");
        assert!(err.to_string().contains("stream"));
        assert!(err.to_string().contains("socket"));
    }

    #[test]
    fn stream_without_consumer_is_rejected() {
        let args = SubscribeArgs {
            stream: Some(name("ORDERS")),
            ..Default::default()
        };
        let err = classify(args).expect_err("half a JetStream binding is illegal");
        assert!(err.to_string().contains("both `stream` and `consumer`"));
    }

    #[test]
    fn consumer_without_stream_is_rejected() {
        let args = SubscribeArgs {
            consumer: Some(name("proc")),
            ..Default::default()
        };
        let err = classify(args).expect_err("half a JetStream binding is illegal");
        assert!(err.to_string().contains("both `stream` and `consumer`"));
    }

    #[test]
    fn auto_ack_without_jetstream_is_rejected() {
        let args = SubscribeArgs {
            auto_ack: Some(Span::call_site()),
            ..Default::default()
        };
        let err = classify(args).expect_err("auto_ack alone is illegal");
        assert!(err.to_string().contains("auto_ack"));
    }

    #[test]
    fn jetstream_rejects_subject() {
        let args = SubscribeArgs {
            stream: Some(name("ORDERS")),
            consumer: Some(name("proc")),
            subject: Some(lit("orders.created")),
            ..Default::default()
        };
        let err = classify(args).expect_err("subject + stream/consumer is illegal");
        assert!(err.to_string().contains("subject"));
    }

    fn jetstream(
        inbox: Option<&str>,
        without: Option<&str>,
        isolation: Option<&str>,
    ) -> SubscribeArgs {
        SubscribeArgs {
            stream: Some(name("DEPOSITS")),
            consumer: Some(name("booker")),
            inbox: inbox.map(lit),
            without_inbox: without.map(lit),
            isolation: isolation.map(|level| syn::Ident::new(level, Span::call_site())),
            ..Default::default()
        }
    }

    #[test]
    fn a_jetstream_subscribe_must_choose_inbox_or_without_inbox() {
        let err = classify(jetstream(None, None, None)).expect_err("no choice");
        assert!(err.to_string().contains("must choose"), "{err}");
    }

    #[test]
    fn inbox_with_an_isolation_level_is_an_inbox_consumer() {
        for level in ["Serializable", "RepeatableRead", "ReadCommitted"] {
            assert!(matches!(
                classify(jetstream(Some("billing.book"), None, Some(level))).expect(level),
                Transport::JetStreamInbox { .. }
            ));
        }
    }

    #[test]
    fn inbox_requires_a_named_isolation_level() {
        let err = classify(jetstream(Some("billing.book"), None, None)).expect_err("no level");
        assert!(err.to_string().contains("isolation"), "{err}");
        let err = classify(jetstream(
            Some("billing.book"),
            None,
            Some("ReadUncommitted"),
        ))
        .expect_err("no dirty reads");
        assert!(err.to_string().contains("`isolation` is"), "{err}");
    }

    #[test]
    fn inbox_rejects_auto_ack_and_without_inbox() {
        let mut args = jetstream(Some("billing.book"), None, Some("Serializable"));
        args.auto_ack = Some(Span::call_site());
        assert!(
            classify(args)
                .expect_err("auto_ack")
                .to_string()
                .contains("auto_ack")
        );
        let err = classify(jetstream(
            Some("billing.book"),
            Some("reason"),
            Some("Serializable"),
        ))
        .expect_err("both");
        assert!(err.to_string().contains("without_inbox"), "{err}");
    }

    #[test]
    fn without_inbox_needs_a_reason_and_no_isolation() {
        assert!(classify(jetstream(None, Some("  "), None)).is_err());
        let err = classify(jetstream(
            None,
            Some("keyed by order id"),
            Some("Serializable"),
        ))
        .expect_err("isolation without an inbox");
        assert!(err.to_string().contains("isolation"), "{err}");
    }

    #[test]
    fn inbox_keys_are_refused_off_jetstream() {
        let args = SubscribeArgs {
            inbox: Some(lit("billing.book")),
            ..Default::default()
        };
        assert!(
            classify(args)
                .expect_err("core")
                .to_string()
                .contains("core NATS")
        );
        let args = SubscribeArgs {
            set: Some(Span::call_site()),
            without_inbox: Some(lit("why")),
            ..Default::default()
        };
        assert!(classify(args).expect_err("set").to_string().contains("set"));
    }
}
