//! Structural reflection of serde wire types for the Fundamental framework.
//!
//! Derive it beside the serde derives:
//!
//! ```
//! use fundamental_schema::{Schema, SchemaNode};
//!
//! #[derive(Schema)]
//! struct CreateUser {
//!     name: String,
//!     age: u8,
//!     nickname: Option<String>,
//! }
//!
//! let SchemaNode::Object { required, .. } = CreateUser::schema() else {
//!     panic!("a struct describes as an object");
//! };
//! // A non-`Option` field is required; an `Option` one is not.
//! assert_eq!(required, ["name", "age"]);
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

#[doc(hidden)]
pub mod describe;
mod impls;
mod schema;
mod schema_node;

// `describe!` is exported at the crate root by `#[macro_export]`; the `describe` module
// is public (hidden) only so the macro expansion can name its probe types.
pub use fundamental_macros::Schema;
pub use schema::Schema;
pub use schema_node::SchemaNode;
