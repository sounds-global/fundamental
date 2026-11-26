//! axum-backed HTTP API surface for the [Fundamental](https://docs.rs/fundamental) framework.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod accept;
mod api_listener;
mod api_server;
mod axum_backend;
mod boxed_verifier;
mod guarded_body;
mod inbound_trace;
mod panic_response;
mod request_span;
mod resolve_request_id;
mod security_header_layer;
mod serve_connections;
mod write_stall_guard;

pub use api_listener::ApiListener;
pub use api_server::ApiServer;
pub use axum_backend::AxumBackend;

pub use fundamental_core::AnyError;

pub use fundamental_server::{
    Admission, AdmissionRequest, BodyPolicy, ContentSecurityPolicy, CspHash, CspSource,
    FrameAncestors, FrameSource, Hsts, HstsMaxAge, HttpContext, InboundRequestId,
    InboundTraceContext, Indexing, Method, Middleware, MiddlewareResult, Next, OpenerPolicy,
    PathParams, Peer, Redirect, ReferrerPolicy, RequestBodyStream, Route, Router, SecurityHeaders,
    ServerConfig, ServerError, ServerListener, ServerResult, StreamBounds, StreamPermit,
    StreamedResponse, auth, decode_body, encode_body,
};
