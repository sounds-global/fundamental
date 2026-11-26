use tracing::subscriber::Interest;
use tracing::{Level, Metadata, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

const TARGET: &str = "async_nats::connection";

/// Refuses every `trace` event from the NATS driver's connection target.
///
/// `async-nats` writes each `CONNECT`, password and token included, at `trace`; a filter
/// directive cannot hold it back, as a span directive admits it, so a layer refuses the callsite.
#[derive(Debug, Clone, Copy, Default)]
pub struct NatsConnectCeiling;

impl NatsConnectCeiling {
    fn refuses(metadata: &Metadata<'_>) -> bool {
        *metadata.level() == Level::TRACE && metadata.target().starts_with(TARGET)
    }
}

impl<S: Subscriber> Layer<S> for NatsConnectCeiling {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if Self::refuses(metadata) {
            Interest::never()
        } else {
            Interest::always()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
        !Self::refuses(metadata)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tracing::{Event, info_span, trace};
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    #[derive(Clone, Default)]
    struct Seen {
        connect: Arc<AtomicUsize>,
        other: Arc<AtomicUsize>,
    }

    impl<S: Subscriber> Layer<S> for Seen {
        fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
            let counter = if event.metadata().target() == TARGET {
                &self.connect
            } else {
                &self.other
            };
            counter.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn a_span_directive_does_not_open_the_connect_line() {
        let seen = Seen::default();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new("trace,[connect]=trace"))
            .with(NatsConnectCeiling)
            .with(vec![
                Box::new(seen.clone()) as Box<dyn Layer<_> + Send + Sync>
            ]);
        tracing::subscriber::with_default(subscriber, || {
            let _span = info_span!("connect").entered();
            trace!(target: "async_nats::connection", pass = "secret", "write operation: CONNECT");
            trace!(target: "async_nats::connector", "another target is untouched");
        });
        assert_eq!(
            seen.connect.load(Ordering::SeqCst),
            0,
            "the CONNECT line was written"
        );
        assert_eq!(
            seen.other.load(Ordering::SeqCst),
            1,
            "other trace events still flow"
        );
    }
}
