use fundamental_observability::trace_context::{
    TraceParent, TraceParentRejection, adopt_trace_parent, link_trace_parent,
};
use tracing::{Span, debug, info};

/// What an inbound W3C `traceparent` may do to the span a request is served in.
///
/// Only the parsed [`TraceParent`] is used; `tracestate` is never taken from a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum InboundTraceContext {
    /// The inbound context is not used at all.
    Ignore,
    /// The request starts its own trace and links the caller's span. The default.
    #[default]
    Link,
    /// The caller's span becomes the parent, for callers of the same deployment.
    Adopt,
}

impl InboundTraceContext {
    /// Meets a request's inbound trace context on `span`, its outermost span, before it is entered.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn meet(self, span: &Span, inbound: Result<Option<TraceParent>, TraceParentRejection>) {
        if let Ok(Some(parent)) = &inbound {
            match self {
                Self::Ignore => {}
                Self::Link => link_trace_parent(span, parent),
                Self::Adopt => adopt_trace_parent(span, parent),
            }
        }
        span.in_scope(|| match inbound {
            Ok(None) => {}
            Ok(Some(_)) => debug!(
                policy = self.class(),
                "met an inbound trace context by policy"
            ),
            Err(rejection) => info!(
                rejected = rejection.class(),
                "an inbound trace context is not valid; the request keeps its own trace"
            ),
        });
    }

    /// The policy as a fixed, lower-case word — the value a log field records.
    #[must_use]
    pub const fn class(self) -> &'static str {
        match self {
            Self::Ignore => "ignore",
            Self::Link => "link",
            Self::Adopt => "adopt",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_a_link() {
        assert_eq!(InboundTraceContext::default(), InboundTraceContext::Link);
        assert_eq!(InboundTraceContext::default().class(), "link");
    }
}
