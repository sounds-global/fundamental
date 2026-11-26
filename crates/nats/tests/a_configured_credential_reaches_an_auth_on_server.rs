//! A NATS credential written in `[messaging.nats]` is the one the connection presents.
//!
//! The server has no anonymous user, so a credential that did not arrive is a refusal.

use fundamental_config::settings::NatsSettings;
use fundamental_core::Environment;
use fundamental_core::Error as _;
use fundamental_nats::{NatsClient, NatsConfig, NatsError, ReconnectConfig};
use fundamental_test_containers::nats::{NatsAuthServer, NatsUser};

/// `[messaging.nats]` for `server`, carrying `credential`, read the way the loader reads it.
fn settings(server: &NatsAuthServer, credential: &str) -> NatsSettings {
    toml::from_str(&format!("url = \"{}\"\n{credential}", server.url()))
        .expect("the section parses")
}

/// The connection a service builds from its settings, reconnection off so a refusal is reported.
async fn config(settings: &NatsSettings) -> NatsConfig {
    NatsConfig::from_settings(settings, Environment::Production, None)
        .await
        .expect("a credential given as a value needs no secret store")
        .with_reconnect(ReconnectConfig::fail_fast())
}

#[tokio::test]
async fn the_configured_password_authenticates_the_connection() {
    let server = NatsAuthServer::shared().await.expect("the auth server");
    let settings = settings(
        server,
        &format!(
            "username = \"{}\"\npassword = \"{}\"\n",
            NatsUser::App.username(),
            NatsUser::App.password().expose()
        ),
    );

    let client = NatsClient::connect(config(&settings).await)
        .await
        .expect("the app user's password lets it in");
    client.flush().await.expect("and the server answers it");
}

#[tokio::test]
async fn a_wrong_password_or_none_is_refused() {
    let server = NatsAuthServer::shared().await.expect("the auth server");
    let wrong = settings(
        server,
        &format!(
            "username = \"{}\"\npassword = \"not-the-password\"\n",
            NatsUser::App.username()
        ),
    );
    let none = settings(server, "");

    for settings in [wrong, none] {
        let refused = NatsClient::connect(config(&settings).await)
            .await
            .expect_err("a server with no anonymous user refuses the connection");
        assert!(
            matches!(refused, NatsError::Authentication(_)),
            "refused for its credential: {refused:?}"
        );
        assert_eq!(refused.error_code(), "NATS_003");
        assert!(
            !refused.is_retryable(),
            "a wrong credential does not heal by waiting"
        );
    }
}
