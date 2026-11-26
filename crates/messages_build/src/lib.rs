//! A crate's messages, built: each message's signature read from the source language's file,
//! and the module of typed functions they are called by.

mod check;
mod error_code_index;
mod functions;
mod generate;
mod module;
mod policy;
mod signatures;
mod texts;

pub use check::{Language, Report, check};
pub use error_code_index::{ErrorCodeIndex, ErrorCodeSite};
pub use functions::{BRAND_TERMS, FUNCTIONS};
pub use generate::{build, generate};
pub use module::module;
pub use policy::{Environment, Finding, Gap, Kind, Level, Policy};
pub use signatures::{Input, InputKind, Signature, signatures};
pub use texts::{Text, texts};
