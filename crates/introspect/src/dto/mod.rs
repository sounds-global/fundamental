//! Owned, round-trippable mirrors of the introspected descriptors, fetched as `surface.json`.

mod action_dto;
mod doc_page_dto;
mod log_record;
mod message_dto;
mod schema_dto;
mod span_record;
mod surface_dto;

pub use action_dto::ActionDto;
pub use doc_page_dto::DocPageDto;
pub use log_record::LogRecord;
pub use message_dto::MessageDto;
pub use schema_dto::{SchemaDto, SchemaField};
pub use span_record::SpanRecord;
pub use surface_dto::SurfaceDto;
