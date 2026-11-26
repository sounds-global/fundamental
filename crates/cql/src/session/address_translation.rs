//! Rewrite the addresses nodes advertise, to targets that may be names.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use scylla::errors::TranslationError;
use scylla::policies::address_translator::{AddressTranslator, UntranslatedPeer};
use tracing::debug;

/// Where a rewrite rule points.
///
/// A name is resolved on every translation, since its address is not ours to choose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TranslationTarget {
    /// A fixed address. Rewritten to exactly this, forever.
    Fixed(SocketAddr),
    /// A name and a port, resolved each time the driver asks.
    ///
    /// The port is kept apart: a resolver returns addresses, and the port is ours.
    Resolved { host: String, port: u16 },
}

impl TranslationTarget {
    /// The address to dial now.
    ///
    /// # Errors
    /// [`TranslationError::IoError`] if the name cannot be resolved or resolves to nothing.
    async fn dial_address(&self) -> Result<SocketAddr, TranslationError> {
        match self {
            Self::Fixed(address) => Ok(*address),
            Self::Resolved { host, port } => {
                let mut resolved = tokio::net::lookup_host((host.as_str(), *port))
                    .await
                    .map_err(|error| TranslationError::IoError(Arc::new(error)))?;
                resolved.next().ok_or_else(|| {
                    TranslationError::IoError(Arc::new(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("'{host}:{port}' resolved to no addresses"),
                    )))
                })
            }
        }
    }
}

/// The driver's address rewrites, with targets that may be names.
///
/// The driver's own translator maps `SocketAddr` to `SocketAddr`; a container's address
/// changes on every recreate, so a named target is looked up at dial time instead.
#[derive(Debug)]
pub(crate) struct AddressTranslation {
    /// Advertised address → where to actually dial.
    rules: HashMap<SocketAddr, TranslationTarget>,
}

impl AddressTranslation {
    pub(crate) const fn new(rules: HashMap<SocketAddr, TranslationTarget>) -> Self {
        Self { rules }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.rules.len()
    }
}

#[async_trait]
impl AddressTranslator for AddressTranslation {
    /// Rewrites `untranslated_peer`'s advertised address, resolving the target if it is a
    /// name.
    ///
    /// An address with no rule is an error, as in the driver's own translator.
    async fn translate_address(
        &self,
        untranslated_peer: &UntranslatedPeer,
    ) -> Result<SocketAddr, TranslationError> {
        let advertised = untranslated_peer.untranslated_address();
        let Some(target) = self.rules.get(&advertised) else {
            return Err(TranslationError::NoRuleForAddress(advertised));
        };
        let dial = target.dial_address().await?;
        debug!(%advertised, %dial, "translated node address");
        Ok(dial)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::SocketAddr;

    use super::TranslationTarget;

    fn address(text: &str) -> SocketAddr {
        text.parse().expect("a socket address")
    }

    #[tokio::test]
    async fn a_fixed_target_is_dialled_verbatim() {
        let target = TranslationTarget::Fixed(address("10.0.0.7:9142"));
        assert_eq!(
            target.dial_address().await.expect("a fixed target"),
            address("10.0.0.7:9142")
        );
    }

    #[tokio::test]
    async fn a_named_target_resolves_and_keeps_its_port() {
        let target = TranslationTarget::Resolved {
            host: "localhost".to_owned(),
            port: 9142,
        };
        let dial = target.dial_address().await.expect("localhost resolves");
        assert_eq!(dial.port(), 9142);
        assert!(dial.ip().is_loopback(), "expected loopback, got {dial}");
    }

    #[tokio::test]
    async fn a_name_that_does_not_resolve_is_an_error() {
        let target = TranslationTarget::Resolved {
            host: "no-such-host.invalid".to_owned(),
            port: 9142,
        };
        target
            .dial_address()
            .await
            .expect_err("`.invalid` never resolves, by RFC 2606");
    }

    #[test]
    fn an_empty_translation_reports_itself_empty() {
        assert!(super::AddressTranslation::new(HashMap::new()).is_empty());
    }
}
