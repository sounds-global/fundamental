//! Identity vocabulary: who the caller is, as pure data.

mod authenticated_principal;
mod device_id;
mod user_id;

pub use authenticated_principal::AuthenticatedPrincipal;
pub use device_id::DeviceId;
pub use user_id::UserId;
