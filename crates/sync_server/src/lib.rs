//! Server-side sync endpoint for the [Fundamental](https://docs.rs/fundamental)
//! framework.
//!
//! [`slice_sync_router`] builds the WebSocket endpoint: it authenticates on the first frame,
//! then serves server-side [slices](SliceRegistry) for the bound principal.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod device_enrollment;
mod device_status;
mod enrollment_registry;
mod enrollment_store;
mod live;
mod memory_enrollment_store;
mod projection;
mod rate_limiter;
mod read_view;
mod signed_endpoint;
mod slice;
mod slice_endpoint;
#[cfg(feature = "sql")]
mod sql_enrollment_store;
mod sync_endpoint;
mod vault;

pub use device_enrollment::DeviceEnrollment;
pub use device_status::DeviceStatus;
pub use enrollment_registry::EnrollmentRegistry;
pub use enrollment_store::EnrollmentStore;
pub use live::LiveSyncServer;
pub use memory_enrollment_store::MemoryEnrollmentStore;
#[cfg(feature = "nats")]
pub use projection::nats_projection_consumer;
pub use projection::{
    ProjectionContext, ProjectionError, ProjectionRegistry, ProjectionUpdate, ProjectionWriter,
    ServerProjection, merge_capture_tables, run_projection_refresh,
};
pub use rate_limiter::RateLimiter;
pub use slice::{
    NoParams, ResolvedSlice, ServerSlice, SliceContext, SliceError, SliceRegistry,
    SliceServerSource,
};
pub use slice_endpoint::{live_slice_sync_router, signed_slice_sync_router, slice_sync_router};
#[cfg(feature = "sql")]
pub use sql_enrollment_store::SqlEnrollmentStore;
pub use vault::{
    VaultChallenge, VaultChallengeGauge, VaultHandshake, VaultPrincipal, authenticate_vault,
    vault_slice_sync_router,
};
