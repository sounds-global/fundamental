//! Example: dynamic, leased database credentials from HashiCorp Vault.
//!
//! Needs a live Vault with the database engine at `database/` and a role `app`; with
//! `enabled: false` it exits cleanly. Run with:
//!   cargo run --example leased_db_credentials -p fundamental_secrets --all-features

use std::time::Duration;

use fundamental_config::settings::{VaultAuth, VaultSettings};
use fundamental_core::secrets::InMemorySecretString;
use fundamental_secrets::{SecretResult, VaultClient, spawn_db_lease_renewer};

/// The database secrets-engine mount and role to mint credentials from.
const DB_MOUNT: &str = "database";
const DB_ROLE: &str = "app";

#[tokio::main]
async fn main() -> SecretResult<()> {
    // 1. Configure the Vault backend (in a service, from `fundamental_config`).
    let settings = VaultSettings {
        // Disabled so the example exits cleanly without a live Vault.
        enabled: false,
        address: "https://127.0.0.1:8200".to_string(),
        namespace: None,
        auth: VaultAuth::Token {
            token: InMemorySecretString::new("s.example-dev-token".to_string()),
        },
        ca_cert_path: None,
        tls_skip_verify: false,
    };

    // 2. Build the client; `Ok(None)` when the backend is disabled.
    let Some(vault) = VaultClient::from_settings(&settings).await? else {
        println!("Vault backend disabled; nothing to do. Set `enabled = true` to run.");
        return Ok(());
    };

    // 3. Mint a short-lived database credential.
    let creds = vault.dynamic_db_creds(DB_MOUNT, DB_ROLE).await?;
    println!(
        "minted lease {} (ttl {}s, renewable {})",
        creds.lease_id, creds.lease_duration, creds.renewable
    );

    // 4. Use the credential.
    let _conn_user = creds.username.expose();
    let _conn_pass = creds.password.expose();

    // 5. Renew the lease once, explicitly.
    if creds.renewable {
        let new_ttl = vault
            .renew_lease(&creds.lease_id, creds.lease_duration)
            .await?;
        println!("renewed lease {}; new ttl {}s", creds.lease_id, new_ttl);
    }

    // 6. Or let the background renewer keep it fresh and rotate it.
    let _renewer = spawn_db_lease_renewer(
        vault.clone(),
        DB_MOUNT.to_string(),
        DB_ROLE.to_string(),
        creds,
        |fresh| async move {
            // Reconnect the pool here; an `Err` keeps the current credential.
            println!("rotated to new lease {}", fresh.lease_id);
            Ok::<(), fundamental_secrets::SecretError>(())
        },
    );

    tokio::time::sleep(Duration::from_secs(60)).await;

    Ok(())
}
