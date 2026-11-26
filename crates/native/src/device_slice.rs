use std::marker::PhantomData;
use std::rc::Rc;

use fundamental_core::AnyError;
use fundamental_offline::db::OfflineDatabase;
use fundamental_offline::store::{OfflineRecord, OfflineStore};
use fundamental_sync::wire::HostedRows;
use fundamental_sync::{Slice, SliceBinding, Syncable, ensure_mirror_table};
use fundamental_ws_client::{SyncClient, SyncClientConfig};
use serde_json::{Map, Value};

use crate::NativeError;

/// Type-erased access to one slice for the sync host.
///
/// Every operation runs through the typed model underneath.
pub trait DeviceSlice: Send + Sync {
    /// The slice's stable name (`Slice::NAME`).
    fn slice(&self) -> &'static str;

    /// The mirror table the slice materializes into (`Syncable::MIRROR_TABLE`).
    fn table(&self) -> &'static str;

    /// Builds the validated binding for `params` (JSON, as the webview sent them).
    ///
    /// # Errors
    /// [`NativeError::SliceParams`] when the parameters do not deserialize or
    /// fail validation.
    fn binding(&self, params: Value) -> Result<SliceBinding, NativeError>;

    /// The binding a parameter-less slice syncs eagerly from app start, or `None` when its
    /// `Params` do not accept an empty object.
    fn eager_binding(&self) -> Option<SliceBinding>;

    /// Starts a sync session for `binding` into `db`.
    ///
    /// Must run inside the host's `LocalSet`; dropping the returned handle
    /// stops the session.
    fn start_session(
        &self,
        config: SyncClientConfig,
        binding: SliceBinding,
        db: Rc<OfflineDatabase>,
    ) -> SyncClient;

    /// Creates the slice's mirror table when absent, so a read never races the first session.
    ///
    /// # Errors
    /// [`NativeError::Other`] when the schema statement fails.
    fn ensure_table(&self, db: &OfflineDatabase) -> Result<(), NativeError>;

    /// Reads the slice's current local rows as the wire payload.
    ///
    /// # Errors
    /// [`NativeError::SyncHostDatabase`] when the mirror read fails.
    fn rows(&self, db: &OfflineDatabase) -> Result<HostedRows, NativeError>;
}

/// The typed [`DeviceSlice`] implementation for slice marker `S`.
pub struct DeviceSliceEntry<S>(PhantomData<fn() -> S>);

impl<S> DeviceSliceEntry<S> {
    /// The entry for slice `S`.
    #[must_use]
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<S> Default for DeviceSliceEntry<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> DeviceSlice for DeviceSliceEntry<S>
where
    S: Slice + 'static,
    S::Model: OfflineRecord + Syncable + 'static,
{
    fn slice(&self) -> &'static str {
        S::NAME
    }

    fn table(&self) -> &'static str {
        <S::Model as Syncable>::MIRROR_TABLE
    }

    fn binding(&self, params: Value) -> Result<SliceBinding, NativeError> {
        let params: S::Params =
            serde_json::from_value(params).map_err(|source| NativeError::SliceParams {
                slice: S::NAME.to_owned(),
                source: fundamental_sync::SliceBindError::Serialize(source.to_string()),
            })?;
        SliceBinding::for_slice::<S>(params).map_err(|source| NativeError::SliceParams {
            slice: S::NAME.to_owned(),
            source,
        })
    }

    fn eager_binding(&self) -> Option<SliceBinding> {
        let empty = Value::Object(Map::new());
        let params: S::Params = serde_json::from_value(empty).ok()?;
        SliceBinding::for_slice::<S>(params).ok()
    }

    fn start_session(
        &self,
        config: SyncClientConfig,
        binding: SliceBinding,
        db: Rc<OfflineDatabase>,
    ) -> SyncClient {
        SyncClient::start::<S::Model>(config, binding, db)
    }

    fn ensure_table(&self, db: &OfflineDatabase) -> Result<(), NativeError> {
        ensure_mirror_table::<S::Model>(db)
            .map_err(|source| NativeError::Other(AnyError::new(source)))
    }

    fn rows(&self, db: &OfflineDatabase) -> Result<HostedRows, NativeError> {
        let records: Vec<S::Model> = OfflineStore::new(db)
            .all::<S::Model>()
            .map_err(|source| NativeError::SyncHostDatabase { source })?;
        Ok(HostedRows::from_records(&records))
    }
}
