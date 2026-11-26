/// The push service a registration token belongs to.
///
/// It travels with the token, since a sender that guesses wrong fails silently. Each token's
/// normative encoding:
///
/// | Variant | The token is |
/// | --- | --- |
/// | [`Apns`](Self::Apns) | the 32-byte device token as lowercase hex, no separators |
/// | [`Fcm`](Self::Fcm) | the registration token verbatim, as the SDK returned it |
/// | [`UnifiedPush`](Self::UnifiedPush) | the distributor's endpoint URL |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PushProvider {
    /// Apple Push Notification service — iOS, iPadOS and macOS.
    Apns,
    /// Firebase Cloud Messaging — Android, and iOS builds that route through it.
    Fcm,
    /// UnifiedPush, for Linux desktops and de-Googled Android.
    UnifiedPush,
}
