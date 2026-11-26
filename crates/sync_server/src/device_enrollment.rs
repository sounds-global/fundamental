use fundamental_auth::UserId;
use fundamental_keys::{DeviceId, VerifyingKey};

use crate::DeviceStatus;

/// One enrolled device public key, bound to the user that owns the device.
///
/// Rotation moves a key to [`DeviceStatus::Grace`] rather than deleting it, so in-flight pushes
/// signed by it keep verifying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEnrollment {
    /// The device this key belongs to.
    pub device_id: DeviceId,
    /// The user the device is bound to.
    pub user_id: UserId,
    /// The device's public key.
    pub public_key: VerifyingKey,
    /// Where the key sits in its lifecycle.
    pub status: DeviceStatus,
    /// When the key was enrolled (ms since the epoch).
    pub enrolled_at: i64,
    /// The deadline after which a `Grace` key stops verifying (ms since the epoch).
    pub grace_until: Option<i64>,
    /// When the key was revoked, if it was (ms since the epoch).
    pub revoked_at: Option<i64>,
}

impl DeviceEnrollment {
    /// Returns whether this key may verify a signature at `now` (ms since epoch).
    #[must_use]
    pub fn is_usable_at(&self, now: i64) -> bool {
        match self.status {
            DeviceStatus::Active => true,
            DeviceStatus::Grace => self.grace_until.is_some_and(|deadline| now < deadline),
            DeviceStatus::Revoked => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enrollment(status: DeviceStatus, grace_until: Option<i64>) -> DeviceEnrollment {
        DeviceEnrollment {
            device_id: DeviceId::new("d"),
            user_id: UserId::new("u"),
            public_key: VerifyingKey::from_bytes([1u8; 32]),
            status,
            enrolled_at: 0,
            grace_until,
            revoked_at: None,
        }
    }

    #[test]
    fn active_is_always_usable() {
        assert!(enrollment(DeviceStatus::Active, None).is_usable_at(1_000));
    }

    #[test]
    fn grace_is_usable_only_before_the_deadline() {
        let e = enrollment(DeviceStatus::Grace, Some(500));
        assert!(e.is_usable_at(499));
        assert!(!e.is_usable_at(500));
        assert!(!e.is_usable_at(501));
    }

    #[test]
    fn revoked_is_never_usable() {
        assert!(!enrollment(DeviceStatus::Revoked, None).is_usable_at(0));
    }
}
