/// The platform's notification authorization state for this app.
///
/// Asking is the app's platform work; "not asked yet" and "refused" stay distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PushAuthorization {
    /// Nobody has been asked yet.
    NotDetermined,
    /// The user refused, or policy forbids it; only the OS settings can undo it.
    Denied,
    /// Granted quietly (Apple's provisional authorization).
    Provisional,
    /// Granted in full.
    Authorized,
}

impl PushAuthorization {
    /// Whether notifications may be delivered at all.
    #[must_use]
    pub fn is_granted(self) -> bool {
        matches!(self, Self::Provisional | Self::Authorized)
    }
}

#[cfg(test)]
mod tests {
    use super::PushAuthorization;

    #[test]
    fn provisional_authorization_still_delivers() {
        assert!(PushAuthorization::Provisional.is_granted());
        assert!(PushAuthorization::Authorized.is_granted());
    }

    #[test]
    fn an_unanswered_prompt_is_not_a_grant() {
        assert!(!PushAuthorization::NotDetermined.is_granted());
        assert!(!PushAuthorization::Denied.is_granted());
    }
}
