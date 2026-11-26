//! [`APP_LISTENERS`] — the tables of an app that bind a socket.

/// The tables of `[apps.<key>]` that bind a socket, each with its own `host`, `port` and `tls`.
pub const APP_LISTENERS: [&str; 5] = ["web", "api", "socket", "sync", "admin"];
