use crate::OpenRefusal;
use fundamental_macros::Error;

/// A [`Keyring`](crate::Keyring) could not be built, could not seal, or refused a token.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KeyringError {
    /// The settings cannot make a keyring. Every problem is listed, not only the first.
    #[error(
        code = "KEYRING_001",
        message = "The {purpose} keyring is misconfigured: {problems}",
        status = 500
    )]
    InvalidSettings {
        /// The keyring's purpose.
        purpose: &'static str,
        /// Every problem found, joined.
        problems: String,
    },

    /// A token was refused; message and code are the same whatever the reason, so the refusal is
    /// no oracle. `reason` is for the operator.
    #[error(
        code = "KEYRING_002",
        message = "The {purpose} token was refused",
        status = 400
    )]
    Refused {
        /// The keyring's purpose.
        purpose: &'static str,
        /// Why — never returned to whoever sent the token.
        reason: OpenRefusal,
    },

    /// A caller asked to seal more than the purpose allows.
    #[error(
        code = "KEYRING_003",
        message = "A {purpose} plaintext of {len} bytes exceeds the {max} the purpose allows",
        status = 500
    )]
    PlaintextTooLong {
        /// The keyring's purpose.
        purpose: &'static str,
        /// The plaintext's length.
        len: usize,
        /// The purpose's `MAX_PLAINTEXT_LEN`.
        max: usize,
    },

    /// The operating system could not supply a nonce. Nothing was sealed.
    #[error(
        code = "KEYRING_004",
        message = "Could not gather entropy to seal a {purpose} token",
        status = 500,
        retryable
    )]
    Entropy {
        /// The keyring's purpose.
        purpose: &'static str,
    },

    /// The cipher refused to seal.
    #[error(
        code = "KEYRING_005",
        message = "Could not seal a {purpose} token",
        status = 500
    )]
    Encryption {
        /// The keyring's purpose.
        purpose: &'static str,
    },

    /// A [`KeyScope`](crate::KeyScope) name outside its closed alphabet.
    #[error(
        code = "KEYRING_006",
        message = "`{scope}` is not a key scope: a lower-case letter, then letters, digits and single hyphens, at most 32",
        status = 500
    )]
    InvalidScope {
        /// The refused name, truncated after the longest accepted length.
        scope: String,
    },

    /// The keyring kept in OpenBao could not be read: the secret is missing or unreachable, its
    /// `current` is not a version, or a version up to it has no material.
    #[error(
        code = "KEYRING_007",
        message = "The {purpose} keyring at {path} cannot be read: {reason}",
        status = 500
    )]
    Unreadable {
        /// The keyring's purpose.
        purpose: &'static str,
        /// The key-value secret it was read from.
        path: String,
        /// What was missing or refused; never a secret's value.
        reason: String,
    },
}
