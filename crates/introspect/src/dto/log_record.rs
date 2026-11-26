use serde::{Deserialize, Serialize};

/// One structured log event captured from a running app's `tracing` output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    /// A monotonic sequence number, unique and increasing within one app run.
    pub seq: u64,
    /// When the event happened, in Unix milliseconds.
    pub timestamp_ms: u64,
    /// The level token (`ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE`).
    pub level: String,
    /// The event's target (the module path it was emitted from).
    pub target: String,
    /// The event's message (its `message` field, or empty).
    pub message: String,
    /// The event's structured fields, in declaration order, rendered to strings.
    #[serde(default)]
    pub fields: Vec<(String, String)>,
    /// The id of the span the event occurred in, if any.
    #[serde(default)]
    pub span: Option<u64>,
}
