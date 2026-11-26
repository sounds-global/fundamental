use fundamental_core::{AnyError, ProbeFuture, ReadinessProbe};

use crate::client::{ConnectionState, NatsClient};
use crate::error::NatsError;

/// Reports a service ready only while its NATS connection is actually usable.
///
/// Register it on the admin surface beside the database probes:
///
/// ```rust,no_run
/// # extern crate fundamental_nats;
/// # use fundamental_nats::{NatsClient, NatsProbe};
/// # fn wire(client: &NatsClient) -> NatsProbe {
/// NatsProbe::new("nats", client)
/// # }
/// ```
///
/// Reconnecting is not ready, since a publish mid-reconnect is buffered with no latency
/// ceiling. A refused subscription is not ready either (`NATS_019`, not retryable). No network
/// round trip.
pub struct NatsProbe {
    name: &'static str,
    client: NatsClient,
}

impl NatsProbe {
    /// Creates a probe reporting on `client` under `name`.
    ///
    /// `name` is what `/ready` returns, so name the connection's role when there are several.
    #[must_use]
    pub fn new(name: &'static str, client: &NatsClient) -> Self {
        Self {
            name,
            client: client.clone(),
        }
    }
}

impl ReadinessProbe for NatsProbe {
    fn name(&self) -> &'static str {
        self.name
    }

    fn check(&self) -> ProbeFuture<'_> {
        Box::pin(async move {
            let state = self.client.state();
            if state != ConnectionState::Connected {
                // Named, so `/ready` tells "reconnecting" from "closed".
                return Err(AnyError::new(NatsError::Connection(state.to_string())));
            }
            if let Some(refused) = self.client.refused_subscription() {
                return Err(AnyError::new(NatsError::SubscriptionRefused {
                    subscription: refused.to_string(),
                }));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    /// The states that must not pass.
    #[test]
    fn only_connected_is_ready() {
        for state in [
            ConnectionState::Disconnected,
            ConnectionState::Connecting,
            ConnectionState::Reconnecting,
            ConnectionState::Closed,
        ] {
            assert_ne!(
                state,
                ConnectionState::Connected,
                "{state} must not be treated as ready"
            );
        }
    }

    /// A failing probe carries a retryable 503.
    #[test]
    fn a_disconnected_state_is_a_retryable_503_naming_the_state() {
        let error = NatsError::Connection(ConnectionState::Reconnecting.to_string());

        assert_eq!(error.error_code(), "NATS_001");
        assert_eq!(error.http_status_code(), 503);
        assert!(error.is_retryable());
        assert!(
            error.display_message().contains("reconnecting"),
            "the state must reach the response; got {}",
            error.display_message()
        );
    }

    /// A refused subscription is a non-retryable 503 naming what was refused.
    #[test]
    fn a_refused_subscription_is_a_definitive_503_naming_the_subscription() {
        let error = NatsError::SubscriptionRefused {
            subscription: "rpc.layer1.attribute.read (queue layer1_attribute_read)".to_string(),
        };

        assert_eq!(error.error_code(), "NATS_019");
        assert_eq!(error.http_status_code(), 503);
        assert!(
            !error.is_retryable(),
            "retrying cannot clear an authorization refusal; only an operator can"
        );
        assert!(
            error
                .display_message()
                .contains("rpc.layer1.attribute.read"),
            "the refused subject must reach the response; got {}",
            error.display_message()
        );
        assert!(
            error.display_message().contains("layer1_attribute_read"),
            "the queue group is half the grant, so it has to be named too; got {}",
            error.display_message()
        );
    }
}
