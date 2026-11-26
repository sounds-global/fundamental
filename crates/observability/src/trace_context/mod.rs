//! Carrying one trace across a process boundary.
//!
//! [`inject_current`] writes the current span's context onto an outgoing carrier;
//! [`adopt_parent`] reads it back at the far end. Transports implement [`Injector`] and
//! [`Extractor`]; without the `otlp` feature both functions are no-ops.
//!
//!
//! ```rust
//! # extern crate fundamental_observability;
//! use fundamental_observability::trace_context::{self, Injector};
//!
//! struct Headers(Vec<(String, String)>);
//!
//! impl Injector for Headers {
//!     fn set(&mut self, key: &str, value: String) {
//!         self.0.retain(|(name, _)| name != key);
//!         self.0.push((key.to_string(), value));
//!     }
//! }
//!
//! let mut headers = Headers(Vec::new());
//! trace_context::inject_current(&mut headers);
//! // `headers` now carries `traceparent` when this process exports traces.
//! ```

#[cfg(feature = "otlp")]
mod adapter;
mod adopt_parent;
mod adopt_trace_parent;
mod extractor;
mod inject_current;
mod injector;
mod link_trace_parent;
#[cfg(feature = "otlp")]
mod remote_span_context;
#[cfg(test)]
mod test_support;
mod trace_parent;
mod trace_parent_rejection;

pub use adopt_parent::adopt_parent;
pub use adopt_trace_parent::adopt_trace_parent;
pub use extractor::Extractor;
pub use inject_current::inject_current;
pub use injector::Injector;
pub use link_trace_parent::link_trace_parent;
#[cfg(feature = "otlp")]
use remote_span_context::remote_span_context;
pub use trace_parent::TraceParent;
pub use trace_parent_rejection::TraceParentRejection;

/// Whether this build carries trace context across process boundaries.
#[must_use]
pub const fn is_propagating() -> bool {
    cfg!(feature = "otlp")
}

#[cfg(test)]
mod tests {
    use super::test_support::Carrier;
    use super::*;

    #[cfg(not(feature = "otlp"))]
    #[test]
    fn without_export_nothing_is_written() {
        let mut carrier = Carrier::default();
        inject_current(&mut carrier);
        assert!(carrier.0.is_empty());
    }

    #[cfg(feature = "otlp")]
    mod exporting {
        use super::super::test_support::{trace_id_of, with_tracing};
        use super::*;

        #[test]
        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn a_context_written_on_one_hop_is_adopted_on_the_next() {
            let (sent, received) = with_tracing(|| {
                let caller = tracing::info_span!("caller");
                let sent = caller.in_scope(|| {
                    let mut carrier = Carrier::default();
                    inject_current(&mut carrier);
                    carrier
                });

                let callee = tracing::info_span!("callee");
                adopt_parent(&callee, &sent);
                let received = trace_id_of(&callee);
                (trace_id_of(&caller), received)
            });

            assert!(sent.is_some(), "the caller's span must carry a trace id");
            assert_eq!(
                sent, received,
                "the callee must continue the caller's trace, not start its own"
            );
        }

        #[test]
        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn injection_writes_the_w3c_header() {
            let carrier = with_tracing(|| {
                tracing::info_span!("caller").in_scope(|| {
                    let mut carrier = Carrier::default();
                    inject_current(&mut carrier);
                    carrier
                })
            });

            let traceparent = carrier
                .get("traceparent")
                .expect("the W3C header names the trace");
            assert!(
                traceparent.starts_with("00-"),
                "version 00 is the only one defined: {traceparent}"
            );
        }

        /// An absent header keeps the parent tracing already gave the span.
        #[test]
        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn an_empty_carrier_leaves_the_local_parent_alone() {
            let (parent, child) = with_tracing(|| {
                let outer = tracing::info_span!("outer");
                outer.in_scope(|| {
                    let inner = tracing::info_span!("inner");
                    adopt_parent(&inner, &Carrier::default());
                    (trace_id_of(&outer), trace_id_of(&inner))
                })
            });

            assert_eq!(
                parent, child,
                "with nothing to adopt, the span keeps the trace it was created in"
            );
        }
    }
}
