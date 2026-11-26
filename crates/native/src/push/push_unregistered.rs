/// Why a device can no longer be reached by push.
///
/// Not a refusal of permission, which is [`PushAuthorization::Denied`](crate::PushAuthorization::Denied).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PushUnregistered {
    /// The platform invalidated the registration; a new token may arrive later.
    Revoked,
    /// Registration failed: the platform refused to issue a token, or the app's
    /// own registrar rejected one.
    Failed {
        /// What failed; never a token.
        detail: String,
    },
}
