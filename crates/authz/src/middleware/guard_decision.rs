//! [`GuardDecision`]: what one permission check decided about one request.

use fundamental_core::{AnyError, Error as _};
use fundamental_server::ServerError;

use super::forbidden::Forbidden;

/// The outcome of a [`GuardCheck`](super::GuardCheck); a check that could not be made is the
/// `Err` beside it, never either answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GuardDecision {
    /// The check allowed: the caller holds the relation on `object_id`.
    Allowed {
        /// The object's canonical id, exactly as checked.
        object_id: String,
    },
    /// The check denied.
    Denied {
        /// Whether the request carried no principal.
        anonymous: bool,
    },
}

impl GuardDecision {
    /// The error a denial is answered with, or `None` for an allow.
    ///
    /// An anonymous caller gets `401`, since it is unidentified rather than forbidden.
    #[must_use]
    pub fn refusal(&self) -> Option<AnyError> {
        match self {
            Self::Allowed { .. } => None,
            Self::Denied { anonymous: true } => Some(AnyError::new(ServerError::unauthenticated())),
            Self::Denied { anonymous: false } => Some(AnyError::new(Forbidden::Denied)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GuardDecision;

    #[test]
    fn a_denial_is_401_anonymous_and_403_otherwise() {
        let anonymous = GuardDecision::Denied { anonymous: true }
            .refusal()
            .expect("a refusal");
        assert_eq!(anonymous.get().http_status_code(), 401);
        let known = GuardDecision::Denied { anonymous: false }
            .refusal()
            .expect("a refusal");
        assert_eq!(known.get().http_status_code(), 403);
        assert_eq!(known.get().error_code(), "AUTHZ_014");
        assert!(
            GuardDecision::Allowed {
                object_id: "7".into()
            }
            .refusal()
            .is_none()
        );
    }
}
