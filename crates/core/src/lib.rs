//! Core traits and primitive types for the Fundamental framework.

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

#[cfg(not(feature = "std"))]
extern crate alloc;

mod sealed {
    pub trait Sealed {}
}
mod command_address;
mod command_address_objection;
mod display_safe;
mod environment;
mod escape;
mod mail_address;
mod mail_address_problem;
mod provider_name;
mod readiness_probe;
mod scope_set;
mod service_declaration;
mod service_state;

#[doc(hidden)]
pub(crate) use sealed::Sealed;

/// Serde, for code `#[derive(CanonicalText)]` generates.
#[doc(hidden)]
pub use serde as __serde;

pub mod action;
pub mod address;
pub mod asset;
pub mod bounded;
pub mod custody;
pub mod env;
pub mod error;
#[cfg(feature = "std")]
pub mod fs;
pub mod i18n;
pub mod identity;
#[cfg(feature = "std")]
pub mod listen;
pub mod model;
pub mod money;
pub mod outbox;
#[cfg(feature = "retry")]
pub mod retry;
pub mod secrets;
#[cfg(feature = "proptest")]
pub mod testing;
pub mod validate;
pub mod variant;

pub use display_safe::is_unsafe_for_display;
pub use error::{AnyError, Error, ErrorOverride};
pub use escape::escape;
pub use identity::{AuthenticatedPrincipal, DeviceId, UserId};
pub use mail_address::MailAddress;
pub use mail_address_problem::MailAddressProblem;
pub use model::{Connection, GeneratedId, Handle, HasHandle, Identifier, Model, Primary};
pub use provider_name::ProviderName;
pub use validate::Validate;
pub use variant::{Device, Platform, Variant};

pub use command_address::CommandAddress;
pub use command_address_objection::CommandAddressObjection;
pub use environment::*;
pub use readiness_probe::{ProbeFuture, ReadinessProbe};
pub use scope_set::ScopeSet;
pub use service_declaration::ServiceDeclaration;
pub use service_state::{Phase, ServiceState};
