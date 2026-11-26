//! The one request/reply round-trip the `fun authz` verbs make to a running service.

use fundamental::nats::{Message, NatsClient, NatsConfig, ReconnectConfig};
use tokio::runtime::Runtime;

use crate::CliError;

/// Sends `request` to the authorization service at `nats_url` on a short-lived runtime.
///
/// # Errors
/// [`CliError::AuthzRuntime`] if the runtime cannot start, [`CliError::Nats`] if the request fails.
pub(super) fn request<Req, Res>(nats_url: &str, request: Req) -> Result<Res, CliError>
where
    Req: Message,
    Res: Message,
{
    let runtime = Runtime::new().map_err(|err| CliError::AuthzRuntime {
        reason: err.to_string(),
    })?;
    runtime.block_on(async move {
        // Fail fast against a missing service rather than retrying.
        let config = NatsConfig::new(nats_url).with_reconnect(ReconnectConfig::fail_fast());
        let client = NatsClient::connect(config).await?;
        let reply = client.request::<Req, Res>(request).await?;
        Ok(reply.payload)
    })
}
