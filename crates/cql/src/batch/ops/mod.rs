//! Concrete batch operation builders.

mod delete;
mod insert;

pub use delete::DeleteBatch;
pub use insert::InsertBatch;
