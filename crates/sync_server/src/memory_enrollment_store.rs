use std::collections::HashMap;
use std::sync::Mutex;

use fundamental_keys::KeyError;

use crate::enrollment_store::EnrollmentStore;
use crate::{DeviceEnrollment, DeviceStatus};

/// An in-memory [`EnrollmentStore`], for a single instance and for tests.
#[derive(Debug, Default)]
pub struct MemoryEnrollmentStore {
    devices: Mutex<HashMap<String, Vec<DeviceEnrollment>>>,
}

impl MemoryEnrollmentStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<DeviceEnrollment>>> {
        self.devices
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl EnrollmentStore for MemoryEnrollmentStore {
    fn load<'a>(
        &'a self,
        device_id: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<DeviceEnrollment>, KeyError>> + Send + 'a>,
    > {
        Box::pin(async move { Ok(self.lock().get(device_id).cloned().unwrap_or_default()) })
    }

    fn insert(
        &self,
        enrollment: DeviceEnrollment,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), KeyError>> + Send + '_>>
    {
        Box::pin(async move {
            self.lock()
                .entry(enrollment.device_id.as_str().to_string())
                .or_default()
                .push(enrollment);
            Ok(())
        })
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn grace_active<'a>(
        &'a self,
        device_id: &'a str,
        grace_until: i64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), KeyError>> + Send + 'a>>
    {
        Box::pin(async move {
            if let Some(list) = self.lock().get_mut(device_id) {
                for enrollment in list.iter_mut() {
                    if enrollment.status == DeviceStatus::Active {
                        enrollment.status = DeviceStatus::Grace;
                        enrollment.grace_until = Some(grace_until);
                    }
                }
            }
            Ok(())
        })
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn revoke_all<'a>(
        &'a self,
        device_id: &'a str,
        revoked_at: i64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), KeyError>> + Send + 'a>>
    {
        Box::pin(async move {
            if let Some(list) = self.lock().get_mut(device_id) {
                for enrollment in list.iter_mut() {
                    enrollment.status = DeviceStatus::Revoked;
                    enrollment.revoked_at = Some(revoked_at);
                }
            }
            Ok(())
        })
    }
}
