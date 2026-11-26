//! The SQL repositories held in memory: a model's rows written, read and queried as
//! YugabyteDB does, for an app's tests in milliseconds.
//!
//! Not supported: `SqlModelQuery`, `update_where`, `Secret` columns and `read_by` row security.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod cells;
mod compare;
mod kinds;
mod matching;
mod memory_database;
mod memory_repository;
mod reading;
mod unique;

pub use memory_database::MemoryDatabase;
pub use memory_repository::{Held, MemoryRepository};
