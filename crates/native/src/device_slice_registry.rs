use std::collections::BTreeMap;

use fundamental_offline::store::OfflineRecord;
use fundamental_sync::{Slice, Syncable};

use crate::NativeError;
use crate::device_slice::{DeviceSlice, DeviceSliceEntry};

/// The typed set of slices a [`SyncHost`](crate::SyncHost) syncs and serves.
///
/// Built with `mount_device_slices! { Mine, Channel }` or by chaining [`with`](Self::with).
#[derive(Default)]
#[must_use = "a registry does nothing until handed to the sync host"]
pub struct DeviceSliceRegistry {
    entries: BTreeMap<&'static str, Box<dyn DeviceSlice>>,
}

impl DeviceSliceRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds slice `S`; registering the same slice twice keeps one entry.
    pub fn with<S>(mut self) -> Self
    where
        S: Slice + 'static,
        S::Model: OfflineRecord + Syncable + 'static,
    {
        self.entries
            .insert(S::NAME, Box::new(DeviceSliceEntry::<S>::new()));
        self
    }

    /// The entry for `slice`.
    ///
    /// # Errors
    /// [`NativeError::UnknownSlice`] when no entry carries that name.
    pub fn get(&self, slice: &str) -> Result<&dyn DeviceSlice, NativeError> {
        self.entries
            .get(slice)
            .map(AsRef::as_ref)
            .ok_or_else(|| NativeError::UnknownSlice {
                slice: slice.to_owned(),
            })
    }

    /// Every registered entry, in stable (name) order.
    pub fn iter(&self) -> impl Iterator<Item = &dyn DeviceSlice> {
        self.entries.values().map(AsRef::as_ref)
    }

    /// The number of registered slices.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl std::fmt::Debug for DeviceSliceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceSliceRegistry")
            .field("slices", &self.entries.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::DeviceSliceRegistry;
    use crate::NativeError;
    use fundamental_core::Error as _;

    #[test]
    fn unknown_slice_is_a_typed_404() {
        let registry = DeviceSliceRegistry::new();
        let Err(err) = registry.get("nope") else {
            panic!("an empty registry must not resolve a slice");
        };
        assert!(matches!(err, NativeError::UnknownSlice { ref slice } if slice == "nope"));
        assert_eq!(err.error_code(), "NATIVE_016");
        assert_eq!(err.http_status_code(), 404);
        assert!(format!("{err}").contains("nope"));
    }

    #[test]
    fn an_empty_registry_reports_itself() {
        let registry = DeviceSliceRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert_eq!(registry.iter().count(), 0);
    }

    // `with::<S>()` is covered by the sync-host integration tests.
}
