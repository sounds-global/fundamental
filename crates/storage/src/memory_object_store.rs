//! An object store held in memory, for tests and previews.

use crate::{ByteRange, ByteStream, ObjectBody, ObjectStore, StoreError};
use bytes::Bytes;
use fundamental_core::ProviderName;
use futures::{StreamExt, stream};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// An [`ObjectStore`] held in memory that can be made unreachable or versioned; clones share one store.
#[derive(Clone, Default)]
pub struct MemoryObjectStore {
    objects: Arc<Mutex<BTreeMap<String, Bytes>>>,
    unreachable: Arc<AtomicBool>,
    versioned: Arc<AtomicBool>,
}

const PROVIDER: ProviderName = ProviderName::new("memory");

impl MemoryObjectStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The bytes held under `key`, exactly as stored.
    #[must_use]
    pub fn raw(&self, key: &str) -> Option<Bytes> {
        self.objects().get(key).cloned()
    }

    /// Replaces the bytes held under `key`, as a provider that altered them would.
    pub fn tamper(&self, key: &str, bytes: Bytes) {
        self.objects().insert(key.to_owned(), bytes);
    }

    /// Every key held.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        self.objects().keys().cloned().collect()
    }

    /// Makes every call fail as an unreachable provider does, or recover.
    pub fn set_unreachable(&self, unreachable: bool) {
        self.unreachable.store(unreachable, Ordering::Release);
    }

    /// Makes the store report that it keeps old versions, which a bucket refuses at boot.
    pub fn set_versioned(&self, versioned: bool) {
        self.versioned.store(versioned, Ordering::Release);
    }

    fn objects(&self) -> MutexGuard<'_, BTreeMap<String, Bytes>> {
        self.objects.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn reachable(&self) -> Result<(), StoreError> {
        if self.unreachable.load(Ordering::Acquire) {
            return Err(StoreError::Unreachable { provider: PROVIDER });
        }
        Ok(())
    }
}

impl ObjectStore for MemoryObjectStore {
    fn provider(&self) -> ProviderName {
        PROVIDER
    }

    async fn put(&self, key: &str, max_len: u64, mut body: ByteStream) -> Result<u64, StoreError> {
        self.reachable()?;
        let mut held = Vec::new();
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            if held.len() as u64 + chunk.len() as u64 > max_len {
                return Err(StoreError::TooLarge { max: max_len });
            }
            held.extend_from_slice(&chunk);
        }
        let len = held.len() as u64;
        self.objects().insert(key.to_owned(), Bytes::from(held));
        Ok(len)
    }

    async fn get(&self, key: &str, range: Option<ByteRange>) -> Result<ObjectBody, StoreError> {
        self.reachable()?;
        let object = self.raw(key).ok_or(StoreError::NotFound)?;
        let object_len = object.len() as u64;
        let bytes = match range {
            None => object,
            Some(range) => {
                let range = range.within(object_len).ok_or(StoreError::Refused {
                    provider: PROVIDER,
                    status: 416,
                })?;
                let first = usize::try_from(range.first()).unwrap_or(usize::MAX);
                let last = usize::try_from(range.last()).unwrap_or(usize::MAX);
                object.slice(first..=last)
            }
        };
        // Yields in small pieces, so a reader that assumes one chunk per segment is caught.
        let pieces: Vec<Result<Bytes, StoreError>> = (0..bytes.len())
            .step_by(4_096)
            .map(|at| Ok(bytes.slice(at..(at + 4_096).min(bytes.len()))))
            .collect();
        Ok(ObjectBody {
            len: bytes.len() as u64,
            object_len,
            stream: Box::pin(stream::iter(pieces)),
        })
    }

    async fn head(&self, key: &str) -> Result<Option<u64>, StoreError> {
        self.reachable()?;
        Ok(self.raw(key).map(|object| object.len() as u64))
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.reachable()?;
        self.objects().remove(key);
        Ok(())
    }

    fn presign_get(&self, key: &str, ttl: Duration) -> Result<String, StoreError> {
        Ok(format!("memory:///{key}?ttl={}", ttl.as_secs()))
    }

    async fn deletes_are_final(&self) -> Result<(), StoreError> {
        self.reachable()?;
        if self.versioned.load(Ordering::Acquire) {
            return Err(StoreError::InvalidSettings {
                problems: "the bucket keeps old versions".to_owned(),
            });
        }
        Ok(())
    }
}
