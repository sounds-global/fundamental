//! Error category marker traits.

mod authentication;
mod authorization;
mod conflict;
mod internal;
mod not_found;

pub use authentication::AuthenticationError;
pub use authorization::AuthorizationError;
pub use conflict::ConflictError;
pub use internal::InternalError;
pub use not_found::NotFoundError;
