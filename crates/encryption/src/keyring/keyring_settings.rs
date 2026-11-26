use crate::KeyringKeySettings;
use serde::Deserialize;

/// The configuration a [`Keyring`](crate::Keyring) is built from.
///
/// Rotation: add the new version to every service that opens, then promote it to `current` on
/// the service that seals; remove an old version only once nothing it sealed can still arrive.
#[derive(Debug, Deserialize)]
pub struct KeyringSettings {
    /// The version new tokens and tags are made with.
    pub current: u16,
    /// Every version this service holds: the current one and any still being opened.
    #[serde(default)]
    pub versions: Vec<KeyringKeySettings>,
}
