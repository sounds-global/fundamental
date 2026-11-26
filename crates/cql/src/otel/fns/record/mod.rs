//! Recorders for YCQL OpenTelemetry tracing spans.

mod lwt_result;
mod outcome;

pub use lwt_result::record_lwt_result;
pub use outcome::record_outcome;
