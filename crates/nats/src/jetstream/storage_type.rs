use async_nats::jetstream;
use serde::{Deserialize, Serialize};

/// Stream storage backend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum StorageType {
    /// File-backed storage.
    #[default]
    File,
    /// In-memory storage.
    Memory,
}

impl From<jetstream::stream::StorageType> for StorageType {
    fn from(value: jetstream::stream::StorageType) -> Self {
        match value {
            jetstream::stream::StorageType::File => Self::File,
            jetstream::stream::StorageType::Memory => Self::Memory,
        }
    }
}

impl From<StorageType> for jetstream::stream::StorageType {
    fn from(value: StorageType) -> Self {
        match value {
            StorageType::File => Self::File,
            StorageType::Memory => Self::Memory,
        }
    }
}
