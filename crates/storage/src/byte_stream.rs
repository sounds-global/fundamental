//! Bytes that arrive over time.

use crate::StoreError;
use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;

/// Bytes arriving a chunk at a time, any of which may fail — an object being stored or read.
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, StoreError>> + Send>>;
