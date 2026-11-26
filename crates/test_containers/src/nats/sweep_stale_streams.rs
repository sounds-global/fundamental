use fundamental_db::ephemeral::{EphemeralName, stale_cutoff_millis};
use fundamental_nats::{NatsClient, NatsConfig, ReconnectConfig};
use tracing::warn;

use crate::test_container_error::TestContainerError;

/// Removes JetStream streams old enough that nobody can still be using them.
///
/// By the age in a name [`NatsServer::stream_name`](super::NatsServer::stream_name) minted, so a
/// concurrent run's live streams survive; anything else is left alone.
///
/// # Errors
/// Returns [`TestContainerError::Provision`] if the server cannot be reached or the listing
/// fails. A stream that resists deletion is reported and skipped.
pub(crate) async fn sweep_stale_streams(url: &str) -> Result<usize, TestContainerError> {
    let provision = |detail: String| TestContainerError::Provision {
        engine: "nats",
        fixture: "the JetStream sweep".to_owned(),
        detail,
    };

    // Unprefixed, so it sees every run's streams.
    let client =
        NatsClient::connect(NatsConfig::new(url).with_reconnect(ReconnectConfig::fail_fast()))
            .await
            .map_err(|error| provision(error.to_string()))?;
    let jetstream = client.jetstream();
    let streams = jetstream
        .list_streams()
        .await
        .map_err(|error| provision(error.to_string()))?;

    let cutoff = stale_cutoff_millis();
    let mut removed = 0;
    for stream in &streams {
        let Some(minted) = EphemeralName::stamp_of(&stream.name) else {
            continue;
        };
        if minted >= cutoff {
            continue;
        }
        match jetstream.delete_stream(&stream.name).await {
            Ok(()) => removed += 1,
            Err(error) => warn!(
                stream = %stream.name, %error,
                "test containers: could not delete a stale JetStream stream"
            ),
        }
    }
    Ok(removed)
}
