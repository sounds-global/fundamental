use serde::{Deserialize, Serialize};

/// One `tracing` span captured from a running app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanRecord {
    /// A monotonic sequence number, unique and increasing within one app run.
    pub seq: u64,
    /// The span's id, unique among live spans.
    pub id: u64,
    /// The parent span's id, if this span was opened inside another.
    #[serde(default)]
    pub parent: Option<u64>,
    /// The span's name.
    pub name: String,
    /// The span's target (the module path it was defined in).
    pub target: String,
    /// When the span opened, in Unix milliseconds.
    pub opened_ms: u64,
    /// When the span closed, in Unix milliseconds — `None` while it is still open.
    #[serde(default)]
    pub closed_ms: Option<u64>,
    /// The fields the span was opened with, in declaration order, rendered to strings.
    #[serde(default)]
    pub fields: Vec<(String, String)>,
}
