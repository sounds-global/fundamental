//! An app's actions and messages as fetchable data: OpenAPI, AsyncAPI and `surface.json`.
//!
//! Without `ssr`, only the target-neutral [`dto`] types; with `ssr`, [`Introspection`] and [`mount`].
//!
//! ```
//! # #[cfg(feature = "ssr")] {
//! use fundamental_introspect::{Introspection, OpenApiDocument};
//! use fundamental_server::{ActionDescriptor, Router};
//! use fundamental_schema::SchemaNode;
//!
//! let actions = vec![ActionDescriptor::new(
//!     // `None`: this act names a single object, so it needs no exemption.
//!     "POST", "/users", "Create a user.", &["auth"], None, SchemaNode::Any, SchemaNode::Any,
//! )];
//! let doc = OpenApiDocument::new("Demo API", "1.0.0", actions.clone()).to_value();
//! assert_eq!(doc["openapi"], "3.1.0");
//!
//! let _router: Router<()> = Introspection::new("Demo API", "1.0.0")
//!     .actions(actions)
//!     .mount(Router::new())
//!     .expect("introspection mounts");
//! # }
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

pub mod dto;

mod paths;
pub use paths::{ASYNCAPI_PATH, LOGS_PATH, OPENAPI_PATH, SURFACE_PATH, TRACES_PATH};

#[cfg(feature = "ssr")]
mod async_api_document;
#[cfg(feature = "ssr")]
mod doc_page;
#[cfg(feature = "ssr")]
mod drain;
#[cfg(feature = "ssr")]
mod mount;
#[cfg(feature = "ssr")]
mod open_api_document;

#[cfg(feature = "ssr")]
pub use async_api_document::AsyncApiDocument;
#[cfg(feature = "ssr")]
pub use doc_page::DocPage;
#[cfg(feature = "ssr")]
pub use drain::Drain;
#[cfg(feature = "ssr")]
pub use mount::{Introspection, mount};
#[cfg(feature = "ssr")]
pub use open_api_document::OpenApiDocument;

#[cfg(feature = "ssr")]
pub use fundamental_nats::{MessageDescriptor, MessageTransport};
#[cfg(feature = "ssr")]
pub use fundamental_server::ActionDescriptor;
