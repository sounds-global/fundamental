//! [`AuthenticatedPrincipal`] — who a verified token says the caller is.

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

use super::{DeviceId, UserId};

/// The verified identity of a caller: the user, optionally the device, the scopes
/// the token grants, and when it expires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedPrincipal {
    /// The authenticated user.
    pub user_id: UserId,
    /// The device the token was issued for, if any.
    pub device_id: Option<DeviceId>,
    /// The scopes the token grants.
    pub scopes: Vec<String>,
    /// Token expiry, in seconds since the Unix epoch.
    pub expires_at: i64,
}

impl AuthenticatedPrincipal {
    /// Returns whether the principal holds `scope`.
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_scope_checks_membership() {
        let principal = AuthenticatedPrincipal {
            user_id: UserId::new("u1"),
            device_id: None,
            scopes: vec!["sync:read".to_string(), "sync:write".to_string()],
            expires_at: 0,
        };
        assert!(principal.has_scope("sync:write"));
        assert!(!principal.has_scope("admin"));
    }

    #[test]
    fn a_principal_can_name_its_device() {
        let principal = AuthenticatedPrincipal {
            user_id: UserId::new("u1"),
            device_id: Some(DeviceId::new("device-7")),
            scopes: Vec::new(),
            expires_at: 0,
        };
        assert_eq!(
            principal.device_id.as_ref().map(DeviceId::as_str),
            Some("device-7")
        );
    }
}
