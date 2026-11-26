use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::{LookupSpan, SpanRef};

use crate::dto::{LogRecord, SpanRecord};

const DEFAULT_CAPACITY: usize = 2000;

/// The namespace every introspection endpoint lives under; a span naming it is dropped.
pub(crate) const INTROSPECT_PREFIX: &str = "/_fundamental/";

/// A bounded, shared capture of a running app's `tracing` events and spans.
///
/// ```no_run
/// # #[cfg(feature = "ssr")] {
/// use fundamental_introspect::{Drain, Introspection};
/// use fundamental_server::Router;
/// use tracing_subscriber::prelude::*;
///
/// let drain = Drain::new();
/// tracing_subscriber::registry()
///     .with(tracing_subscriber::fmt::layer())
///     .with(drain.clone())
///     .init();
/// let router: Router<()> = Introspection::new("demo", "1.0.0")
///     .drain(drain)
///     .mount(Router::new())
///     .expect("introspection mounts");
/// # let _ = router;
/// # }
/// ```
///
/// A request span naming an introspection path is dropped with its subtree, so polling the
/// drain never fills it.
#[derive(Clone)]
pub struct Drain {
    inner: Arc<Inner>,
}

struct Inner {
    logs: Mutex<VecDeque<LogRecord>>,
    spans: Mutex<VecDeque<SpanRecord>>,
    capacity: usize,
    seq: AtomicU64,
}

impl Drain {
    /// Creates a drain with the default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Creates a drain keeping at most `capacity` recent events and spans (at least 1).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                logs: Mutex::new(VecDeque::new()),
                spans: Mutex::new(VecDeque::new()),
                capacity: capacity.max(1),
                seq: AtomicU64::new(0),
            }),
        }
    }

    /// A snapshot of the buffered log events, oldest first.
    #[must_use]
    pub fn logs(&self) -> Vec<LogRecord> {
        self.inner
            .logs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// A snapshot of the buffered spans, oldest first.
    #[must_use]
    pub fn spans(&self) -> Vec<SpanRecord> {
        self.inner
            .spans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    fn next_seq(&self) -> u64 {
        self.inner.seq.fetch_add(1, Ordering::Relaxed)
    }

    fn push_log(&self, record: LogRecord) {
        let mut logs = self
            .inner
            .logs
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if logs.len() >= self.inner.capacity {
            logs.pop_front();
        }
        logs.push_back(record);
    }

    fn push_span(&self, record: SpanRecord) {
        let mut spans = self
            .inner
            .spans
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if spans.len() >= self.inner.capacity {
            spans.pop_front();
        }
        spans.push_back(record);
    }

    fn close_span(&self, id: u64, closed_ms: u64) {
        let mut spans = self
            .inner
            .spans
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(span) = spans
            .iter_mut()
            .rev()
            .find(|s| s.id == id && s.closed_ms.is_none())
        {
            span.closed_ms = Some(closed_ms);
        }
    }

    /// Removes the newest still-open span with `id`; `tracing` reuses a closed span's id.
    fn drop_span(&self, id: u64) {
        let mut spans = self
            .inner
            .spans
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(position) = spans
            .iter()
            .rposition(|span| span.id == id && span.closed_ms.is_none())
        {
            spans.remove(position);
        }
    }

    /// Merges fields recorded after a span opened into the stored span.
    fn record_span_fields(&self, id: u64, updates: Vec<(String, String)>) {
        let mut spans = self
            .inner
            .spans
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(span) = spans
            .iter_mut()
            .rev()
            .find(|s| s.id == id && s.closed_ms.is_none())
        {
            for (name, value) in updates {
                match span.fields.iter_mut().find(|(k, _)| *k == name) {
                    Some(existing) => existing.1 = value,
                    None => span.fields.push((name, value)),
                }
            }
        }
    }
}

impl Default for Drain {
    fn default() -> Self {
        Self::new()
    }
}

// Nothing here logs (that would recurse); a poisoned buffer keeps its last good contents.
impl<S> Layer<S> for Drain
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let meta = attrs.metadata();
        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);
        // A request span naming an introspection path, and every span beneath it, is suppressed.
        let names_introspection = visitor
            .fields
            .iter()
            .any(|(name, value)| is_path_field(name) && value.contains(INTROSPECT_PREFIX));
        let parent_suppressed = ctx
            .span(id)
            .and_then(|span| span.parent())
            .is_some_and(|parent| parent.extensions().get::<Suppressed>().is_some());
        if names_introspection || parent_suppressed {
            if let Some(span) = ctx.span(id) {
                span.extensions_mut().insert(Suppressed);
            }
            return;
        }
        let parent = if let Some(parent) = attrs.parent() {
            Some(parent.into_u64())
        } else if attrs.is_contextual() {
            ctx.current_span().id().map(Id::into_u64)
        } else {
            None
        };
        self.push_span(SpanRecord {
            seq: self.next_seq(),
            id: id.into_u64(),
            parent,
            name: meta.name().to_string(),
            target: meta.target().to_string(),
            opened_ms: now_ms(),
            closed_ms: None,
            fields: visitor.fields,
        });
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if is_suppressed(ctx.event_span(event).as_ref()) {
            return;
        }
        let meta = event.metadata();
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let span = ctx.event_span(event).map(|span| span.id().into_u64());
        self.push_log(LogRecord {
            seq: self.next_seq(),
            timestamp_ms: now_ms(),
            level: meta.level().to_string(),
            target: meta.target().to_string(),
            message: visitor.message.unwrap_or_default(),
            fields: visitor.fields,
            span,
        });
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        // A suppressed introspection span was never stored, so there is nothing to update.
        if is_suppressed(ctx.span(id).as_ref()) {
            return;
        }
        // Fields declared `Empty` at span open and filled later arrive here, not at `on_new_span`.
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);
        // The matched route is known only after routing, so the span was already stored: remove it.
        if visitor
            .fields
            .iter()
            .any(|(name, value)| is_path_field(name) && value.contains(INTROSPECT_PREFIX))
        {
            if let Some(span) = ctx.span(id) {
                span.extensions_mut().insert(Suppressed);
            }
            self.drop_span(id.into_u64());
            return;
        }
        if !visitor.fields.is_empty() {
            self.record_span_fields(id.into_u64(), visitor.fields);
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        // A suppressed introspection span was never stored, so skip the buffer scan to close it.
        if is_suppressed(ctx.span(&id).as_ref()) {
            return;
        }
        self.close_span(id.into_u64(), now_ms());
    }
}

/// Marks a span of the dev-tools' own poll so its whole subtree is dropped.
struct Suppressed;

fn is_suppressed<S>(span: Option<&SpanRef<'_, S>>) -> bool
where
    S: for<'a> LookupSpan<'a>,
{
    span.is_some_and(|span| span.extensions().get::<Suppressed>().is_some())
}

/// Whether a span field names an HTTP path or route (`route`, `path`, `uri`, `http.*`).
fn is_path_field(name: &str) -> bool {
    matches!(
        name,
        "path" | "route" | "uri" | "http.uri" | "http.target" | "http.route"
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Collects a `tracing` event's or span's fields, pulling `message` out separately.
#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}

impl FieldVisitor {
    fn put(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            self.fields.push((field.name().to_string(), value));
        }
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.put(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::{Level, info, info_span};
    use tracing_subscriber::prelude::*;

    #[test]
    fn captures_events_and_spans_and_links_them() {
        let drain = Drain::new();
        let subscriber = tracing_subscriber::registry().with(drain.clone());
        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!("handle_request", route = "/users");
            let _guard = span.enter();
            info!(user_id = 7, "created");
        });

        let logs = drain.logs();
        let created = logs
            .iter()
            .find(|l| l.message == "created")
            .expect("event captured");
        assert_eq!(created.level, Level::INFO.to_string());
        assert!(
            created
                .fields
                .iter()
                .any(|(k, v)| k == "user_id" && v == "7")
        );
        assert!(created.span.is_some(), "event carries its span id");

        let spans = drain.spans();
        let span = spans
            .iter()
            .find(|s| s.name == "handle_request")
            .expect("span captured");
        assert_eq!(created.span, Some(span.id), "the log links to its span");
        assert!(
            span.fields
                .iter()
                .any(|(k, v)| k == "route" && v == "/users")
        );
        assert!(span.closed_ms.is_some(), "the span closed");
    }

    #[test]
    fn captures_span_fields_recorded_after_the_span_opens() {
        use tracing::field::Empty;
        let drain = Drain::new();
        let subscriber = tracing_subscriber::registry().with(drain.clone());
        tracing::subscriber::with_default(subscriber, || {
            // The taught pattern: declare a field `Empty`, fill it in once known.
            let span = info_span!("handle_request", user_id = Empty);
            span.record("user_id", 42);
        });
        let spans = drain.spans();
        let span = spans
            .iter()
            .find(|s| s.name == "handle_request")
            .expect("span captured");
        assert!(
            span.fields.iter().any(|(k, v)| k == "user_id" && v == "42"),
            "a field recorded after open must be captured, got {:?}",
            span.fields
        );
    }

    #[test]
    fn drops_the_dev_tools_own_introspection_polls() {
        let drain = Drain::new();
        let subscriber = tracing_subscriber::registry().with(drain.clone());
        tracing::subscriber::with_default(subscriber, || {
            {
                let span = info_span!("server.dispatch", method = "GET", path = "/users");
                let _guard = span.enter();
                info!("handled");
            }
            // A poll of the traces endpoint is dropped with every span and event beneath it.
            {
                let span = info_span!(
                    "server.dispatch",
                    method = "GET",
                    path = "/_fundamental/traces.json"
                );
                let _guard = span.enter();
                let child = info_span!("snapshot");
                let _child_guard = child.enter();
                info!("served the trace buffer");
            }
            {
                let span = info_span!("request", method = "GET", uri = "/_fundamental/logs.json");
                let _guard = span.enter();
                info!("served the log buffer");
            }
        });

        let spans = drain.spans();
        let names: Vec<&str> = spans.iter().map(|s| s.name.as_str()).collect();
        assert!(
            spans
                .iter()
                .any(|s| s.fields.iter().any(|(k, v)| k == "path" && v == "/users")),
            "the normal request span is kept, got {names:?}"
        );
        assert!(
            !spans
                .iter()
                .any(|s| s.fields.iter().any(|(_, v)| v.contains("/_fundamental/"))),
            "no introspection request span is captured, got {names:?}"
        );
        assert!(
            !spans.iter().any(|s| s.name == "snapshot"),
            "a child span of an introspection poll is dropped too, got {names:?}"
        );

        let logs = drain.logs();
        let messages: Vec<&str> = logs.iter().map(|l| l.message.as_str()).collect();
        assert!(
            messages.contains(&"handled"),
            "the normal event is kept, got {messages:?}"
        );
        assert!(
            !messages.contains(&"served the trace buffer")
                && !messages.contains(&"served the log buffer"),
            "events inside an introspection poll are dropped, got {messages:?}"
        );
    }

    #[test]
    fn suppressing_a_poll_leaves_the_closed_history_that_shares_its_span_id() {
        // `tracing` reuses a closed span's id, so the buffer can hold a closed record under the live id.
        let drain = Drain::new();
        let record = |id, name: &str, closed_ms| SpanRecord {
            seq: 0,
            id,
            parent: None,
            name: name.to_string(),
            target: "test".to_string(),
            opened_ms: 0,
            closed_ms,
            fields: Vec::new(),
        };
        drain.push_span(record(7, "an earlier request", Some(1)));
        drain.push_span(record(7, "the dev-tools poll", None));
        drain.push_span(record(9, "an unrelated request", Some(2)));

        drain.drop_span(7);

        let names: Vec<String> = drain.spans().iter().map(|s| s.name.clone()).collect();
        assert!(
            !names.iter().any(|n| n == "the dev-tools poll"),
            "the open span being suppressed is removed, got {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "an earlier request"),
            "a closed span that merely shares the reused id survives, got {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "an unrelated request"),
            "a span with a different id is untouched, got {names:?}"
        );
    }

    #[test]
    fn the_buffer_is_bounded() {
        let drain = Drain::with_capacity(4);
        let subscriber = tracing_subscriber::registry().with(drain.clone());
        tracing::subscriber::with_default(subscriber, || {
            for i in 0..10 {
                info!(i, "tick");
            }
        });
        let logs = drain.logs();
        assert_eq!(logs.len(), 4, "kept only the most recent capacity");
        assert!(logs.first().unwrap().seq < logs.last().unwrap().seq);
    }
}
