use fundamental_core::secrets::SecretString;

use crate::{PushEnvironment, PushProvider};

/// One device's push registration: the token, the service that issued it, and
/// the environment it is valid in.
///
/// The token is a [`SecretString`]. Not `Serialize`, since that would send `"[REDACTED]"`;
/// equality is hand-written, comparing the token in constant time.
#[derive(Debug, Clone)]
pub struct PushRegistration {
    provider: PushProvider,
    token: SecretString,
    environment: Option<PushEnvironment>,
}

impl Eq for PushRegistration {}

impl PartialEq for PushRegistration {
    fn eq(&self, other: &Self) -> bool {
        use subtle::ConstantTimeEq;

        // Cheap public fields first; `ct_eq` is constant-time only over equal lengths.
        if self.provider != other.provider || self.environment != other.environment {
            return false;
        }

        let held = self.token.expose().as_bytes();
        let offered = other.token.expose().as_bytes();
        held.len() == offered.len() && bool::from(held.ct_eq(offered))
    }
}

impl PushRegistration {
    /// Builds a registration from the platform's token and its provider.
    #[must_use]
    pub fn new(provider: PushProvider, token: SecretString) -> Self {
        Self {
            provider,
            token,
            environment: None,
        }
    }

    /// Records which environment issued the token (APNs sandbox or production).
    #[must_use]
    pub fn with_environment(mut self, environment: PushEnvironment) -> Self {
        self.environment = Some(environment);
        self
    }

    /// The service that issued the token, which decides how to send to it.
    #[must_use]
    pub fn provider(&self) -> PushProvider {
        self.provider
    }

    /// The registration token, still wrapped.
    #[must_use]
    pub fn token(&self) -> &SecretString {
        &self.token
    }

    /// The environment the token is valid in, where the service has more than one.
    #[must_use]
    pub fn environment(&self) -> Option<PushEnvironment> {
        self.environment
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::secrets::SecretString;

    use super::PushRegistration;
    use crate::{PushEnvironment, PushProvider};

    fn registration() -> PushRegistration {
        PushRegistration::new(
            PushProvider::Apns,
            SecretString::new("7a3f00ddeadbeef1".to_owned()),
        )
    }

    #[test]
    fn debug_never_reveals_the_token() {
        let debug = format!("{:?}", registration());
        assert!(
            !debug.contains("7a3f00ddeadbeef1"),
            "a push token must not reach a log through Debug: {debug}"
        );
        assert!(debug.contains("REDACTED"), "{debug}");
    }

    #[test]
    fn the_environment_is_part_of_the_identity() {
        let sandbox = registration().with_environment(PushEnvironment::Sandbox);
        let production = registration().with_environment(PushEnvironment::Production);
        assert_ne!(
            sandbox, production,
            "the same token in two environments reaches two different hosts"
        );
        assert_eq!(sandbox.environment(), Some(PushEnvironment::Sandbox));
    }

    #[test]
    fn a_registration_without_an_environment_is_not_the_same_as_one_with() {
        assert_ne!(
            registration(),
            registration().with_environment(PushEnvironment::Production)
        );
        assert_eq!(registration().environment(), None);
    }

    #[test]
    fn the_token_is_readable_where_a_sender_needs_it() {
        assert_eq!(registration().token().expose(), "7a3f00ddeadbeef1");
        assert_eq!(registration().provider(), PushProvider::Apns);
    }
}
