//! The provider port.

use crate::{ByteRange, ByteStream, ObjectBody, StoreError};
use fundamental_core::ProviderName;
use std::future::Future;
use std::time::Duration;

/// An object store: the port every provider adapter implements.
///
/// The contract, which [`Conformance`](crate::Conformance) checks: a put is whole or absent, a
/// delete is final and idempotent, and a provider's own words never cross as a [`StoreError`].
pub trait ObjectStore: Send + Sync + 'static {
    /// The provider's name, recorded with every object it holds.
    fn provider(&self) -> ProviderName;

    /// Stores `body` under `key`, refusing more than `max_len` bytes. Answers the bytes stored.
    ///
    /// # Errors
    /// [`StoreError::TooLarge`], [`StoreError::BodyFailed`], or the provider's refusal.
    fn put(
        &self,
        key: &str,
        max_len: u64,
        body: ByteStream,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;

    /// Reads the object under `key`, or the bytes of it `range` names.
    ///
    /// # Errors
    /// [`StoreError::NotFound`], or the provider's refusal.
    fn get(
        &self,
        key: &str,
        range: Option<ByteRange>,
    ) -> impl Future<Output = Result<ObjectBody, StoreError>> + Send;

    /// The length of the object under `key`, or `None` if there is none.
    ///
    /// # Errors
    /// The provider's refusal.
    fn head(&self, key: &str) -> impl Future<Output = Result<Option<u64>, StoreError>> + Send;

    /// Deletes the object under `key`. An absent object is already deleted.
    ///
    /// # Errors
    /// The provider's refusal.
    fn delete(&self, key: &str) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// A URL that reads the object under `key` for `ttl`, with no other credential.
    ///
    /// # Errors
    /// [`StoreError::InvalidSettings`] if the store cannot sign one.
    fn presign_get(&self, key: &str, ttl: Duration) -> Result<String, StoreError>;

    /// Refuses a store that keeps what it deletes, such as versioning or an object lock.
    ///
    /// # Errors
    /// [`StoreError::InvalidSettings`] naming what would keep it, or the provider's refusal.
    fn deletes_are_final(&self) -> impl Future<Output = Result<(), StoreError>> + Send;
}
