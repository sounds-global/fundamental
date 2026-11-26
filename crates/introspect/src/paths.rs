//! The introspection endpoint paths, shared by the serving app and the fetching dev-tools.

/// The path one app's whole surface (actions, messages, and authored pages) is served at.
pub const SURFACE_PATH: &str = "/_fundamental/surface.json";
/// The path the OpenAPI 3.1 document is served at.
pub const OPENAPI_PATH: &str = "/_fundamental/openapi.json";
/// The path the AsyncAPI 3.0 document is served at.
pub const ASYNCAPI_PATH: &str = "/_fundamental/asyncapi.json";
/// The path the recent captured log events are served at (the whole bounded buffer per poll).
pub const LOGS_PATH: &str = "/_fundamental/logs.json";
/// The path the recent captured spans are served at (the whole bounded buffer per poll).
pub const TRACES_PATH: &str = "/_fundamental/traces.json";
