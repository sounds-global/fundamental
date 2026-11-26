use fundamental_core::secrets::SecretString;
use fundamental_ws_client::SyncClientConfig;

fn main() {
    let _config = SyncClientConfig::new("wss://hub.example/ws", SecretString::new("t".into()))
        .with_reconnect_max_attempts(Some(10));
}
