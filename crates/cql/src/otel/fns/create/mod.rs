//! Constructors for YCQL OpenTelemetry tracing spans.

mod batch_span;
mod db_span;
mod lwt_span;

pub use batch_span::create_batch_span;
pub use db_span::create_db_span;
pub use lwt_span::create_lwt_span;
