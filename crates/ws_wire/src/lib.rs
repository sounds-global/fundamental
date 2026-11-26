//! The WebSocket wire envelope for the [Fundamental](https://docs.rs/fundamental) framework.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod ws_envelope;

pub use ws_envelope::WsEnvelope;
