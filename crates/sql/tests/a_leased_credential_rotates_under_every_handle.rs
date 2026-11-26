//! A pool on a leased credential rotates under every handle, and no credential outlives its use.

use std::collections::BTreeSet;
use std::time::Duration;

use fundamental_config::settings::{
    CredentialSource, DatabaseSettings, SqlSettings, VaultAuth, VaultSettings,
};
use fundamental_core::Environment;
use fundamental_core::Error as _;
use fundamental_core::secrets::InMemorySecret;
use fundamental_secrets::{SecretError, VaultClient, spawn_db_lease_renewer};
use fundamental_sql::sqlx::Row;
use fundamental_sql::{SqlConfig, SqlConnections, SqlPool};
use fundamental_test_containers::container_name::OPENBAO_02_LEASES;
use fundamental_test_containers::openbao::OpenBaoServer;
use fundamental_test_containers::sql::SqlAuthNode;
use serde_json::json;
use tracing_test::traced_test;

/// Short enough to rotate inside a test, leaving the third of a lease a production TTL leaves.
const DEFAULT_TTL: &str = "6s";
const MAX_TTL: &str = "12s";

const PATIENCE: Duration = Duration::from_secs(40);

struct Engine {
    bao: &'static OpenBaoServer,
    node: &'static SqlAuthNode,
    mount: String,
    root: VaultClient,
}

impl Engine {
    async fn up() -> Self {
        Self::with_leases(DEFAULT_TTL, MAX_TTL).await
    }

    async fn with_leases(default_ttl: &str, max_ttl: &str) -> Self {
        let node = SqlAuthNode::shared().await.expect("the auth node");
        let bao = OpenBaoServer::shared(OPENBAO_02_LEASES)
            .await
            .expect("the leases server");
        let mount = format!("db-{}", uuid::Uuid::new_v4().simple());
        bao.enable_engine(&mount, "database")
            .await
            .expect("a database engine");
        let host = node.bridge_ip().expect("the node has a bridge address");
        let configured = bao
            .post(
                &format!("{mount}/config/ysql"),
                &json!({
                    "plugin_name": "postgresql-database-plugin",
                    "connection_url": format!(
                        "postgresql://{{{{username}}}}:{{{{password}}}}@{host}:5433/yugabyte?sslmode=disable"
                    ),
                    "username": "yugabyte",
                    "password": node.superuser_password(),
                    "allowed_roles": "*",
                }),
            )
            .await
            .expect("the engine is configured");
        assert!(
            configured.status().is_success(),
            "the engine reaches the node: {}",
            configured.text().await.unwrap_or_default()
        );
        for role in ["app", "other"] {
            let created = bao
                .post(
                    &format!("{mount}/roles/{role}"),
                    &json!({
                        "db_name": "ysql",
                        "creation_statements": [
                            "CREATE ROLE \"{{name}}\" WITH LOGIN PASSWORD '{{password}}';",
                        ],
                        "default_ttl": default_ttl,
                        "max_ttl": max_ttl,
                    }),
                )
                .await
                .expect("a role");
            assert!(created.status().is_success(), "role {role} is created");
        }
        let root = vault_as(bao, bao.root_token()).await;
        Self {
            bao,
            node,
            mount,
            root,
        }
    }

    fn settings(&self) -> DatabaseSettings {
        DatabaseSettings {
            sql: Some(SqlSettings {
                url_template: Some(format!(
                    "postgresql://127.0.0.1:{}/yugabyte?sslmode=disable",
                    self.node.port()
                )),
                credentials: CredentialSource::VaultDynamic {
                    mount: self.mount.clone(),
                    role: "app".to_owned(),
                },
                acquire_timeout_secs: Some(5),
                ..SqlSettings::default()
            }),
            ..DatabaseSettings::default()
        }
    }

    /// Every login role on the node now, retried when the read loses a catalog race.
    async fn roles(&self) -> BTreeSet<String> {
        let admin = SqlPool::connect(&SqlConfig::development(self.node.superuser_url().as_str()))
            .await
            .expect("the superuser connects");
        let mut attempts = 0;
        loop {
            match admin
                .fetch_all("SELECT rolname FROM pg_roles WHERE rolname LIKE 'v-%'")
                .await
            {
                Ok(rows) => return rows.iter().map(|row| row.get::<String, _>(0)).collect(),
                Err(error) if error.is_retryable() && attempts < 10 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => panic!("the roles are listed: {error:?}"),
            }
        }
    }
}

async fn vault_as(bao: &OpenBaoServer, token: &str) -> VaultClient {
    VaultClient::from_settings(&VaultSettings {
        enabled: true,
        address: bao.address().to_owned(),
        auth: VaultAuth::Token {
            token: InMemorySecret::new(token.to_owned()),
        },
        ..VaultSettings::default()
    })
    .await
    .expect("a client")
    .expect("enabled")
}

async fn current_user(pool: &SqlPool) -> String {
    pool.fetch_all("SELECT current_user")
        .await
        .expect("the pool serves")
        .first()
        .map(|row| row.get::<String, _>(0))
        .expect("one row")
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while tokio::time::Instant::now() < deadline {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("{what} did not happen within {}s", PATIENCE.as_secs());
}

#[tokio::test]
#[traced_test]
async fn a_handle_taken_before_the_rotation_serves_on_the_new_credential() {
    let engine = Engine::up().await;
    let connections = SqlConnections::from_settings(
        &engine.settings(),
        Environment::Development,
        Some(&engine.root),
    )
    .await
    .expect("the registry connects on a leased credential");
    let early = connections
        .pool_for_domain("default")
        .expect("the default pool");
    let first = current_user(&early).await;

    eventually("a rotation reaching the handle taken before it", || {
        let early = early.clone();
        let first = first.clone();
        async move { current_user(&early).await != first }
    })
    .await;

    eventually("the renewer revoking the lease it replaced", || async {
        logs_contain("revoked vault db lease")
    })
    .await;
    assert!(
        !engine.roles().await.contains(&first),
        "the replaced role is gone once its lease is revoked"
    );
    assert!(logs_contain(
        "vault db lease is near its maximum TTL; rotating before it expires"
    ));
}

#[tokio::test]
#[traced_test]
async fn closing_the_registry_revokes_the_credential_it_ran_on() {
    let engine = Engine::up().await;
    let connections = SqlConnections::from_settings(
        &engine.settings(),
        Environment::Development,
        Some(&engine.root),
    )
    .await
    .expect("the registry connects on a leased credential");
    let pool = connections
        .pool_for_domain("default")
        .expect("the default pool");
    let user = current_user(&pool).await;
    drop(pool);

    connections.close().await;

    assert!(
        !engine.roles().await.contains(&user),
        "the credential ends with the process that used it"
    );
    assert!(logs_contain("the credential in use at shutdown"));
}

#[tokio::test]
#[traced_test]
async fn a_boot_that_cannot_connect_revokes_the_credential_it_minted() {
    let engine = Engine::up().await;
    let mut settings = engine.settings();
    if let Some(sql) = settings.sql.as_mut() {
        sql.url_template = Some("postgresql://127.0.0.1:1/yugabyte?sslmode=disable".to_owned());
        sql.acquire_timeout_secs = Some(1);
    }

    SqlConnections::from_settings(&settings, Environment::Development, Some(&engine.root))
        .await
        .expect_err("no pool connects");

    assert!(logs_contain("the credential no pool could connect with"));
    assert!(
        logs_contain("revoked vault db lease"),
        "the unused credential's lease is revoked, not left to its TTL"
    );
}

#[derive(Debug, fundamental_macros::Error)]
enum Refused {
    #[error(
        code = "TEST_ROT_001",
        message = "the pool refused the new credential",
        status = 503
    )]
    Rebuild,
}

#[tokio::test]
#[traced_test]
async fn a_failed_rebuild_revokes_what_it_minted_and_keeps_the_current_credential() {
    let engine = Engine::up().await;
    let initial = engine
        .root
        .dynamic_db_creds(&engine.mount, "app")
        .await
        .expect("an initial credential");
    let current = initial.username.expose().clone();
    let (minted_tx, mut minted_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let _renewer = spawn_db_lease_renewer(
        engine.root.clone(),
        engine.mount.clone(),
        "app".to_owned(),
        initial,
        move |fresh| {
            let minted_tx = minted_tx.clone();
            async move {
                let _ = minted_tx.send(fresh.username.expose().clone());
                Err::<(), Refused>(Refused::Rebuild)
            }
        },
    );

    let minted = tokio::time::timeout(PATIENCE, minted_rx.recv())
        .await
        .expect("a rotation was attempted")
        .expect("the renewer is running");
    assert_ne!(minted, current);
    eventually("the unused credential's revocation", || {
        let minted = minted.clone();
        let engine = &engine;
        async move { !engine.roles().await.contains(&minted) }
    })
    .await;
    assert!(
        engine.roles().await.contains(&current),
        "the credential in use is not revoked because its replacement failed"
    );
    assert!(logs_contain("TEST_ROT_001"));
    assert!(logs_contain(
        "the pool was not rebuilt on the rotated credential"
    ));
}

#[tokio::test]
async fn a_token_scoped_to_its_role_revokes_its_own_leases_and_no_one_elses() {
    let engine = Engine::up().await;
    let policy = format!(
        "path \"{mount}/creds/app\" {{ capabilities = [\"read\"] }}\n\
         path \"sys/leases/revoke/{mount}/creds/app/*\" {{ capabilities = [\"update\"] }}\n",
        mount = engine.mount
    );
    let name = format!("{}-app", engine.mount);
    engine
        .bao
        .write_policy(&name, &policy)
        .await
        .expect("a policy");
    let token = engine
        .bao
        .token_with_policies(&[&name])
        .await
        .expect("a scoped token");
    let scoped = vault_as(engine.bao, token.expose()).await;

    let own = scoped
        .dynamic_db_creds(&engine.mount, "app")
        .await
        .expect("its own role's credential");
    let neighbour = engine
        .root
        .dynamic_db_creds(&engine.mount, "other")
        .await
        .expect("another role's credential");

    let refused = scoped
        .revoke_lease(&neighbour.lease_id)
        .await
        .expect_err("another role's lease is not this token's to revoke");
    assert!(
        matches!(refused, SecretError::Api { status: 403, .. }),
        "{refused:?}"
    );
    assert_eq!(refused.error_code(), "SECRETS_004");
    assert!(
        engine.roles().await.contains(neighbour.username.expose()),
        "the neighbour's credential survives"
    );

    scoped
        .revoke_lease(&own.lease_id)
        .await
        .expect("its own lease is");
    assert!(!engine.roles().await.contains(own.username.expose()));
}

/// Granted six seconds, renewable up to twelve, so the eighth second's renewal falls short.
const TOKEN_TTL: &str = "6s";
const TOKEN_MAX_TTL: &str = "12s";
const TOKEN_CEILING: Duration = Duration::from_secs(12);

const LONG_LEASE: &str = "1h";

impl Engine {
    async fn approle_client(&self) -> VaultClient {
        let enabled = self
            .bao
            .post("sys/auth/approle", &json!({ "type": "approle" }))
            .await
            .expect("the approle method answers");
        let answer = enabled.text().await.unwrap_or_default();
        assert!(
            answer.is_empty() || answer.contains("already in use"),
            "the approle method is enabled: {answer}"
        );
        let name = format!("{}-service", self.mount);
        let policy = format!(
            "path \"{mount}/creds/app\" {{ capabilities = [\"read\"] }}\n\
             path \"sys/leases/renew/{mount}/creds/app/*\" {{ capabilities = [\"update\"] }}\n\
             path \"sys/leases/revoke/{mount}/creds/app/*\" {{ capabilities = [\"update\"] }}\n",
            mount = self.mount
        );
        self.bao
            .write_policy(&name, &policy)
            .await
            .expect("a policy");
        let role = format!("auth/approle/role/{name}");
        let created = self
            .bao
            .post(
                &role,
                &json!({
                    "token_policies": name,
                    "token_ttl": TOKEN_TTL,
                    "token_max_ttl": TOKEN_MAX_TTL,
                }),
            )
            .await
            .expect("an approle role");
        assert!(created.status().is_success(), "the approle role is created");
        let fixed = self
            .bao
            .post(&format!("{role}/role-id"), &json!({ "role_id": name }))
            .await
            .expect("a role id");
        assert!(fixed.status().is_success(), "the role id is set");
        let secret: serde_json::Value = self
            .bao
            .post(&format!("{role}/secret-id"), &json!({}))
            .await
            .expect("a secret id")
            .json()
            .await
            .expect("a secret id answer");
        let secret_id = secret
            .pointer("/data/secret_id")
            .and_then(serde_json::Value::as_str)
            .expect("the secret id")
            .to_owned();
        VaultClient::from_settings(&VaultSettings {
            enabled: true,
            address: self.bao.address().to_owned(),
            auth: VaultAuth::AppRole {
                role_id: name,
                secret_id: InMemorySecret::new(secret_id),
            },
            ..VaultSettings::default()
        })
        .await
        .expect("an approle login")
        .expect("enabled")
    }
}

#[tokio::test]
#[traced_test]
async fn a_renewal_granted_short_of_its_ask_logs_in_while_the_old_token_lives() {
    let engine = Engine::with_leases(LONG_LEASE, LONG_LEASE).await;
    let vault = engine.approle_client().await;

    tokio::time::sleep(TOKEN_CEILING + Duration::from_secs(3)).await;

    assert!(logs_contain(
        "vault client token is near its maximum TTL; logging in before it expires"
    ));
    assert!(logs_contain("vault client logged in again"));
    assert!(
        !logs_contain("vault token renewal failed"),
        "the old token was replaced before it died, not after"
    );
    vault
        .dynamic_db_creds(&engine.mount, "app")
        .await
        .expect("the client serves past its first token's ceiling");
}

#[tokio::test]
#[traced_test]
async fn a_new_login_rotates_the_credential_before_the_old_token_expires() {
    let engine = Engine::with_leases(LONG_LEASE, LONG_LEASE).await;
    let logged_in = tokio::time::Instant::now();
    let vault = engine.approle_client().await;
    let connections =
        SqlConnections::from_settings(&engine.settings(), Environment::Development, Some(&vault))
            .await
            .expect("the registry connects on a leased credential");
    let pool = connections
        .pool_for_domain("default")
        .expect("the default pool");
    let first = current_user(&pool).await;

    tokio::time::timeout_at(logged_in + TOKEN_CEILING, async {
        while current_user(&pool).await == first {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("the credential rotated onto the new token before the old token expired");
    assert!(logs_contain(
        "rotating the db credential onto the new token"
    ));
}

#[tokio::test]
#[traced_test]
async fn a_query_running_across_the_hand_over_completes_and_the_old_login_ends_after_it() {
    let engine = Engine::with_leases(LONG_LEASE, LONG_LEASE).await;
    let logged_in = tokio::time::Instant::now();
    let vault = engine.approle_client().await;
    let connections =
        SqlConnections::from_settings(&engine.settings(), Environment::Development, Some(&vault))
            .await
            .expect("the registry connects on a leased credential");
    let pool = connections
        .pool_for_domain("default")
        .expect("the default pool");
    let first = current_user(&pool).await;

    // Spans the new login; a revocation before the drain would end this query.
    pool.fetch_all("SELECT pg_sleep(9)")
        .await
        .expect("a query running across the hand-over completes");

    // Gone before the ceiling, so the renewer revoked it.
    tokio::time::timeout_at(logged_in + TOKEN_CEILING, async {
        while engine.roles().await.contains(&first) {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("the replaced login is revoked once its pool closed, before the old token expires");
    assert!(!logs_contain("were still in use when the drain ended"));

    tokio::time::sleep_until(logged_in + TOKEN_CEILING + Duration::from_secs(2)).await;
    assert_ne!(
        current_user(&pool).await,
        first,
        "the pool serves past the old token's ceiling, on the new credential"
    );
}
