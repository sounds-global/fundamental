//! [`IntoSubject`]: the subject inputs the ergonomic API accepts.

use super::SubjectRef;
use crate::error::AuthzError;

/// A value that can be resolved into a [`SubjectRef`].
///
/// A [`SubjectRef`], or with the `server` feature a request's
/// `fundamental_auth::AuthenticatedPrincipal` as `user:<user_id>`. Not an intra-doc link:
/// `fundamental_auth` is optional and the link would break the default doc build.
pub trait IntoSubject {
    /// Resolves `self` into a [`SubjectRef`].
    ///
    /// # Errors
    ///
    /// Returns an [`AuthzError`] if the value cannot form a valid subject.
    fn into_subject(self) -> Result<SubjectRef, AuthzError>;
}

impl IntoSubject for SubjectRef {
    fn into_subject(self) -> Result<SubjectRef, AuthzError> {
        Ok(self)
    }
}

/// Lets the fallible typed constructors be passed directly; the error surfaces in the operation.
impl IntoSubject for Result<SubjectRef, AuthzError> {
    fn into_subject(self) -> Result<SubjectRef, AuthzError> {
        self
    }
}

/// A request principal is the `user:<user_id>` leaf subject.
#[cfg(feature = "server")]
impl IntoSubject for &fundamental_auth::AuthenticatedPrincipal {
    fn into_subject(self) -> Result<SubjectRef, AuthzError> {
        use super::CanonicalId;
        use crate::schema::NamespaceName;

        Ok(SubjectRef::leaf(
            NamespaceName::new("user").expect("`user` is a valid namespace identifier"),
            CanonicalId::new(self.user_id.as_str())?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::NamespaceName;
    use crate::tuple::CanonicalId;

    #[test]
    fn an_existing_subject_passes_through() {
        let subject = SubjectRef::leaf(
            NamespaceName::new("user").unwrap(),
            CanonicalId::new("alice").unwrap(),
        );
        assert_eq!(subject.clone().into_subject().unwrap(), subject);
    }

    #[cfg(feature = "server")]
    #[test]
    fn a_principal_becomes_the_user_leaf_subject() {
        use fundamental_auth::{AuthenticatedPrincipal, UserId};

        let principal = AuthenticatedPrincipal {
            user_id: UserId::new("alice"),
            device_id: None,
            scopes: Vec::new(),
            expires_at: 0,
        };
        let subject = (&principal).into_subject().expect("valid principal");
        assert!(!subject.is_userset());
        assert_eq!(subject.object.namespace.as_str(), "user");
        assert_eq!(subject.object.id.as_str(), "alice");
    }
}
