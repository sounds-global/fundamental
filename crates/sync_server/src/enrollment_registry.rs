use fundamental_auth::UserId;
use fundamental_keys::{DeviceId, DeviceKeyFingerprint, KeyError, Signature, VerifyingKey};
use tracing::{info, warn};

use crate::enrollment_store::EnrollmentStore;
use crate::memory_enrollment_store::MemoryEnrollmentStore;
use crate::{DeviceEnrollment, DeviceStatus};

/// Which device keys are enrolled to which users, and the verifier of their push signatures.
///
/// A device id is the client's own string, so it is never logged; a device is recorded by its
/// key's [`DeviceKeyFingerprint`]. Times are milliseconds since the epoch.
pub struct EnrollmentRegistry {
    store: Box<dyn EnrollmentStore>,
}

impl Default for EnrollmentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl EnrollmentRegistry {
    /// Creates a registry backed by an in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::with_store(Box::new(MemoryEnrollmentStore::new()))
    }

    /// Creates a registry backed by `store`.
    #[must_use]
    pub fn with_store(store: Box<dyn EnrollmentStore>) -> Self {
        Self { store }
    }

    /// Returns the user a device is bound to, if any non-revoked key remains.
    ///
    /// # Errors
    /// Returns a [`KeyError`] if the store cannot be read.
    pub async fn owner(&self, device_id: &DeviceId) -> Result<Option<UserId>, KeyError> {
        let list = self.store.load(device_id.as_str()).await?;
        Ok(list
            .into_iter()
            .find(|e| e.status != DeviceStatus::Revoked)
            .map(|e| e.user_id))
    }

    /// Enrolls `public_key` for `device_id`, bound to `user_id`, as an active key.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if the device is already bound to a different
    /// user, or a [`KeyError`] if the store cannot be read or written.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub async fn enroll(
        &self,
        user_id: UserId,
        device_id: DeviceId,
        public_key: VerifyingKey,
        now: i64,
    ) -> Result<(), KeyError> {
        let existing = self.store.load(device_id.as_str()).await?;
        if let Some(bound) = existing.iter().find(|e| e.status != DeviceStatus::Revoked)
            && bound.user_id != user_id
        {
            warn!("enroll rejected: device bound to another user");
            return Err(KeyError::Malformed(
                "device is already enrolled to another user".to_string(),
            ));
        }
        info!(
            device_key = %DeviceKeyFingerprint::of(&public_key),
            user = user_id.as_str(),
            "device enrolled"
        );
        self.store
            .insert(DeviceEnrollment {
                device_id,
                user_id,
                public_key,
                status: DeviceStatus::Active,
                enrolled_at: now,
                grace_until: None,
                revoked_at: None,
            })
            .await
    }

    /// Rotates `device_id` to `new_public_key`: the new key becomes active and the
    /// current active key enters a grace window of `grace_ms`.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if the device has no active key to rotate,
    /// or a [`KeyError`] if the store cannot be read or written.
    pub async fn rotate(
        &self,
        device_id: &DeviceId,
        new_public_key: VerifyingKey,
        now: i64,
        grace_ms: i64,
    ) -> Result<(), KeyError> {
        let existing = self.store.load(device_id.as_str()).await?;
        let fingerprint = DeviceKeyFingerprint::of(&new_public_key);
        let user_id = existing
            .iter()
            .find(|e| e.status == DeviceStatus::Active)
            .map(|e| e.user_id.clone())
            .ok_or_else(|| KeyError::Malformed("device has no active key to rotate".to_string()))?;

        self.store
            .grace_active(device_id.as_str(), now + grace_ms)
            .await?;
        self.store
            .insert(DeviceEnrollment {
                device_id: device_id.clone(),
                user_id,
                public_key: new_public_key,
                status: DeviceStatus::Active,
                enrolled_at: now,
                grace_until: None,
                revoked_at: None,
            })
            .await?;
        info!(
            device_key = %fingerprint,
            grace_until = now + grace_ms,
            "device key rotated"
        );
        Ok(())
    }

    /// Revokes every key for `device_id` immediately.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if the device is not enrolled, or a
    /// [`KeyError`] if the store cannot be read or written.
    pub async fn revoke(&self, device_id: &DeviceId, now: i64) -> Result<(), KeyError> {
        let enrolled = self.store.load(device_id.as_str()).await?;
        let Some(newest) = enrolled.iter().max_by_key(|e| e.enrolled_at) else {
            return Err(KeyError::Malformed("unknown device".to_owned()));
        };
        let fingerprint = DeviceKeyFingerprint::of(&newest.public_key);
        self.store.revoke_all(device_id.as_str(), now).await?;
        info!(device_key = %fingerprint, keys = enrolled.len(), "device revoked");
        Ok(())
    }

    /// Verifies a device push from `device_id` for `expected_user`, returning the fingerprint of
    /// the key that verified it.
    ///
    /// # Errors
    /// Returns [`KeyError::BadSignature`] if no usable key bound to `expected_user`
    /// verifies the signature, or another [`KeyError`] if the store cannot be read.
    #[expect(
        clippy::cognitive_complexity,
        clippy::too_many_arguments,
        reason = "readability baseline"
    )]
    pub async fn verify_push(
        &self,
        device_id: &DeviceId,
        expected_user: &UserId,
        message: &[u8],
        signature: &Signature,
        now: i64,
    ) -> Result<DeviceKeyFingerprint, KeyError> {
        let list = self.store.load(device_id.as_str()).await?;
        if list.is_empty() {
            warn!("push from an unenrolled device");
            return Err(KeyError::BadSignature);
        }
        for enrollment in &list {
            if &enrollment.user_id != expected_user || !enrollment.is_usable_at(now) {
                continue;
            }
            if enrollment.public_key.verify(message, signature).is_ok() {
                return Ok(DeviceKeyFingerprint::of(&enrollment.public_key));
            }
        }
        warn!(
            user = expected_user.as_str(),
            "device push signature did not verify"
        );
        Err(KeyError::BadSignature)
    }
}

#[cfg(test)]
mod tests {
    use fundamental_keys::SigningKey;

    use super::*;

    fn user() -> UserId {
        UserId::new("alice")
    }
    fn device() -> DeviceId {
        DeviceId::new("device-1")
    }

    #[tokio::test]
    async fn enrolled_device_push_verifies() {
        let registry = EnrollmentRegistry::new();
        let key = SigningKey::generate().expect("generate");
        registry
            .enroll(user(), device(), key.verifying_key(), 0)
            .await
            .expect("enroll");
        let sig = key.sign(b"canonical");
        registry
            .verify_push(&device(), &user(), b"canonical", &sig, 1)
            .await
            .expect("verifies");
    }

    #[tokio::test]
    async fn tampered_message_is_rejected() {
        let registry = EnrollmentRegistry::new();
        let key = SigningKey::generate().expect("generate");
        registry
            .enroll(user(), device(), key.verifying_key(), 0)
            .await
            .expect("enroll");
        let sig = key.sign(b"canonical");
        let err = registry
            .verify_push(&device(), &user(), b"TAMPERED", &sig, 1)
            .await
            .expect_err("must reject");
        assert!(matches!(err, KeyError::BadSignature));
    }

    #[tokio::test]
    async fn a_different_users_session_cannot_use_the_device() {
        let registry = EnrollmentRegistry::new();
        let key = SigningKey::generate().expect("generate");
        registry
            .enroll(user(), device(), key.verifying_key(), 0)
            .await
            .expect("enroll");
        let sig = key.sign(b"canonical");
        let err = registry
            .verify_push(&device(), &UserId::new("mallory"), b"canonical", &sig, 1)
            .await
            .expect_err("must reject");
        assert!(matches!(err, KeyError::BadSignature));
    }

    #[tokio::test]
    async fn unenrolled_device_is_rejected() {
        let registry = EnrollmentRegistry::new();
        let key = SigningKey::generate().expect("generate");
        let sig = key.sign(b"canonical");
        assert!(matches!(
            registry
                .verify_push(&device(), &user(), b"canonical", &sig, 1)
                .await,
            Err(KeyError::BadSignature)
        ));
    }

    #[tokio::test]
    async fn revoked_device_push_is_rejected() {
        let registry = EnrollmentRegistry::new();
        let key = SigningKey::generate().expect("generate");
        registry
            .enroll(user(), device(), key.verifying_key(), 0)
            .await
            .expect("enroll");
        registry.revoke(&device(), 5).await.expect("revoke");
        let sig = key.sign(b"canonical");
        assert!(matches!(
            registry
                .verify_push(&device(), &user(), b"canonical", &sig, 6)
                .await,
            Err(KeyError::BadSignature)
        ));
    }

    #[tokio::test]
    async fn rotation_honors_the_grace_window() {
        let registry = EnrollmentRegistry::new();
        let old = SigningKey::generate().expect("generate");
        let new = SigningKey::generate().expect("generate");
        registry
            .enroll(user(), device(), old.verifying_key(), 0)
            .await
            .expect("enroll");
        registry
            .rotate(&device(), new.verifying_key(), 10, 100)
            .await
            .expect("rotate");

        let old_sig = old.sign(b"canonical");
        let new_sig = new.sign(b"canonical");

        registry
            .verify_push(&device(), &user(), b"canonical", &old_sig, 50)
            .await
            .expect("old key in grace");
        registry
            .verify_push(&device(), &user(), b"canonical", &new_sig, 50)
            .await
            .expect("new key active");

        assert!(matches!(
            registry
                .verify_push(&device(), &user(), b"canonical", &old_sig, 200)
                .await,
            Err(KeyError::BadSignature)
        ));
        registry
            .verify_push(&device(), &user(), b"canonical", &new_sig, 200)
            .await
            .expect("new key still active after grace");
    }

    #[tokio::test]
    async fn rotating_an_unknown_device_errors() {
        let registry = EnrollmentRegistry::new();
        let key = SigningKey::generate().expect("generate");
        assert!(matches!(
            registry
                .rotate(&device(), key.verifying_key(), 0, 100)
                .await,
            Err(KeyError::Malformed(_))
        ));
    }
}
