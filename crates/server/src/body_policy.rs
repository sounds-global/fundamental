use crate::StreamBounds;

/// How the backend reads the body of a request a route will serve, decided before a byte is read.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum BodyPolicy {
    /// Read whole, under [`max_body_size`](crate::ServerConfig::max_body_size), and handed over as bytes.
    Buffered,
    /// Read as it arrives, under these bounds, and handed only to the handler.
    Streamed(StreamBounds),
}
