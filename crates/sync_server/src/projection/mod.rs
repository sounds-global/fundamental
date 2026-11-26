mod merge_capture_tables;
mod projection_context;
mod projection_error;
mod projection_registry;
mod projection_update;
mod projection_writer;
mod run_projection_refresh;
mod server_projection;

pub use merge_capture_tables::merge_capture_tables;
pub use projection_context::ProjectionContext;
pub use projection_error::ProjectionError;
pub use projection_registry::ProjectionRegistry;
pub use projection_update::ProjectionUpdate;
pub use projection_writer::ProjectionWriter;
pub use run_projection_refresh::run_projection_refresh;
pub use server_projection::ServerProjection;

#[cfg(feature = "nats")]
mod nats_projection_consumer;
#[cfg(feature = "nats")]
pub use nats_projection_consumer::nats_projection_consumer;
