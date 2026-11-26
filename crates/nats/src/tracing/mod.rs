//! Distributed-trace propagation through NATS message headers.

mod delivery_ids;
mod nats_trace_context;

pub(crate) use delivery_ids::DeliveryIds;

pub use nats_trace_context::{
    HEADER_PARENT_SPAN_ID, HEADER_REQUEST_ID, HEADER_SESSION_ID, HEADER_SPAN_ID, HEADER_TENANT_ID,
    HEADER_TRACE_ID, HEADER_TRACEPARENT, HEADER_USER_ID, NatsTraceContext, create_child_context,
    extract_trace_context, inject_trace_context,
};
