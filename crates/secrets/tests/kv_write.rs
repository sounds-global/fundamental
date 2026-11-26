//! Key-value (version 2) writes, reads of named fields and destroys against a real OpenBao.
//!
//! Needs Docker, and fails, never skips, without it.

use fundamental_config::settings::{VaultAuth, VaultSettings};
use fundamental_core::Error as _;
use fundamental_core::secrets::InMemorySecret;
use fundamental_secrets::{
    KV_BODY_LIMIT, KvCas, KvData, KvPath, KvVersion, SecretError, VaultClient,
};
use fundamental_test_containers::container_name::OPENBAO_04_KV;
use fundamental_test_containers::openbao::OpenBaoServer;

const MOUNT: &str = "kv-suite";

async fn server() -> &'static OpenBaoServer {
    let server = OpenBaoServer::shared(OPENBAO_04_KV)
        .await
        .expect("an OpenBao container (is Docker running?)");
    server.enable_kv(MOUNT).await.expect("kv mounted");
    server
}

async fn client_with(server: &OpenBaoServer, token: String) -> VaultClient {
    let settings = VaultSettings {
        enabled: true,
        address: server.address().to_owned(),
        auth: VaultAuth::Token {
            token: InMemorySecret::new(token),
        },
        ..VaultSettings::default()
    };
    VaultClient::from_settings(&settings)
        .await
        .expect("client")
        .expect("enabled")
}

async fn root() -> VaultClient {
    let server = server().await;
    client_with(server, server.root_token().to_owned()).await
}

fn fresh_path() -> KvPath {
    KvPath::new(MOUNT, &format!("suite/{}", uuid::Uuid::new_v4().simple())).expect("valid")
}

fn version(n: u64) -> KvVersion {
    KvVersion::new(n).expect("non-zero")
}

#[tokio::test]
async fn a_write_creates_a_version_whose_named_fields_read_back() {
    let vault = root().await;
    let path = fresh_path();
    let data =
        KvData::new(&[("user", "u-1"), ("password", "p-1"), ("_entry", "e-1")]).expect("valid");
    let written = vault
        .write_kv(&path, &data, KvCas::Absent)
        .await
        .expect("written");
    assert_eq!(written.version, version(1));

    let fields = vault
        .read_kv_fields(&path, written.version, &["password", "_entry"])
        .await
        .expect("read");
    assert_eq!(fields.get("password"), Some("p-1"));
    assert_eq!(fields.get("_entry"), Some("e-1"));
    assert_eq!(
        fields.get("user"),
        None,
        "a field not asked for is not read"
    );
}

#[tokio::test]
async fn each_write_reads_back_at_its_own_version() {
    let vault = root().await;
    let path = fresh_path();
    let first = KvData::new(&[("k", "one")]).expect("valid");
    let second = KvData::new(&[("k", "two")]).expect("valid");
    vault
        .write_kv(&path, &first, KvCas::Absent)
        .await
        .expect("first");
    let written = vault
        .write_kv(&path, &second, KvCas::At(version(1)))
        .await
        .expect("second");
    assert_eq!(written.version, version(2));
    let read = |n| vault.read_kv_fields(&path, version(n), &["k"]);
    assert_eq!(read(1).await.expect("v1").get("k"), Some("one"));
    assert_eq!(read(2).await.expect("v2").get("k"), Some("two"));
}

#[tokio::test]
async fn absent_refuses_a_secret_that_exists() {
    let vault = root().await;
    let path = fresh_path();
    let data = KvData::new(&[("k", "v")]).expect("valid");
    vault
        .write_kv(&path, &data, KvCas::Absent)
        .await
        .expect("first");
    let refused = vault
        .write_kv(&path, &data, KvCas::Absent)
        .await
        .expect_err("a second create");
    assert!(
        matches!(refused, SecretError::CasMismatch { .. }),
        "{refused:?}"
    );
    assert_eq!(refused.error_code(), "SECRETS_009");
}

#[tokio::test]
async fn at_refuses_a_version_that_has_moved_on() {
    let vault = root().await;
    let path = fresh_path();
    let data = KvData::new(&[("k", "v")]).expect("valid");
    vault
        .write_kv(&path, &data, KvCas::Absent)
        .await
        .expect("v1");
    vault
        .write_kv(&path, &data, KvCas::At(version(1)))
        .await
        .expect("v2");
    let refused = vault
        .write_kv(&path, &data, KvCas::At(version(1)))
        .await
        .expect_err("a stale writer");
    assert!(
        matches!(refused, SecretError::CasMismatch { .. }),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_write_past_the_limit_is_refused_and_nothing_is_stored() {
    let vault = root().await;
    let path = fresh_path();
    let large = "x".repeat(KV_BODY_LIMIT);
    let data = KvData::new(&[("k", large.as_str())]).expect("valid");
    let refused = vault
        .write_kv(&path, &data, KvCas::Unchecked)
        .await
        .expect_err("too large");
    assert!(matches!(refused, SecretError::WriteTooLarge { limit, .. } if limit == KV_BODY_LIMIT));
    assert_eq!(refused.http_status_code(), 413);
    let nothing = vault.read_kv_fields(&path, version(1), &["k"]).await;
    assert!(
        matches!(nothing, Err(SecretError::VersionGone { version: 1, .. })),
        "{nothing:?}"
    );
}

#[tokio::test]
async fn a_version_never_written_is_gone() {
    let vault = root().await;
    let gone = vault
        .read_kv_fields(&fresh_path(), version(3), &["k"])
        .await
        .expect_err("nothing there");
    assert!(
        matches!(gone, SecretError::VersionGone { version: 3, .. }),
        "{gone:?}"
    );
    assert_eq!(gone.error_code(), "SECRETS_010");
}

#[tokio::test]
async fn a_destroyed_version_is_gone_and_its_neighbour_is_not() {
    let vault = root().await;
    let path = fresh_path();
    let data = KvData::new(&[("k", "v")]).expect("valid");
    vault
        .write_kv(&path, &data, KvCas::Absent)
        .await
        .expect("v1");
    vault
        .write_kv(&path, &data, KvCas::At(version(1)))
        .await
        .expect("v2");
    vault
        .destroy_kv_version(&path, version(1))
        .await
        .expect("destroyed");
    vault
        .destroy_kv_version(&path, version(1))
        .await
        .expect("destroying again succeeds");
    let gone = vault.read_kv_fields(&path, version(1), &["k"]).await;
    assert!(
        matches!(gone, Err(SecretError::VersionGone { .. })),
        "{gone:?}"
    );
    assert!(
        vault
            .read_kv_fields(&path, version(2), &["k"])
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn the_current_version_counts_a_destroyed_one_and_ends_with_the_metadata() {
    let vault = root().await;
    let path = fresh_path();
    assert_eq!(
        vault.read_kv_current_version(&path).await.expect("read"),
        None
    );
    let data = KvData::new(&[("k", "v")]).expect("valid");
    vault
        .write_kv(&path, &data, KvCas::Absent)
        .await
        .expect("v1");
    vault
        .write_kv(&path, &data, KvCas::At(version(1)))
        .await
        .expect("v2");
    vault
        .destroy_kv_version(&path, version(2))
        .await
        .expect("v2 destroyed");
    assert_eq!(
        vault.read_kv_current_version(&path).await.expect("read"),
        Some(version(2))
    );
    vault.destroy_kv(&path).await.expect("destroyed");
    assert_eq!(
        vault.read_kv_current_version(&path).await.expect("read"),
        None
    );
}

#[tokio::test]
async fn destroying_a_secret_takes_every_version_and_is_idempotent() {
    let vault = root().await;
    let path = fresh_path();
    let data = KvData::new(&[("k", "v")]).expect("valid");
    vault
        .write_kv(&path, &data, KvCas::Absent)
        .await
        .expect("v1");
    vault
        .write_kv(&path, &data, KvCas::At(version(1)))
        .await
        .expect("v2");
    vault.destroy_kv(&path).await.expect("destroyed");
    vault
        .destroy_kv(&path)
        .await
        .expect("destroying again succeeds");
    for n in [1, 2] {
        let gone = vault.read_kv_fields(&path, version(n), &["k"]).await;
        assert!(
            matches!(gone, Err(SecretError::VersionGone { .. })),
            "{gone:?}"
        );
    }
    vault
        .write_kv(&path, &data, KvCas::Absent)
        .await
        .expect("the path is free again");
}

#[tokio::test]
async fn a_read_only_token_is_refused_a_write_and_a_write_only_token_a_read() {
    let server = server().await;
    let vault = root().await;
    let path = fresh_path();
    let data = KvData::new(&[("k", "v")]).expect("valid");
    vault
        .write_kv(&path, &data, KvCas::Absent)
        .await
        .expect("v1");

    let data_path = format!("{MOUNT}/data/suite/*");
    server
        .write_policy(
            "kv-suite-read",
            &format!(r#"path "{data_path}" {{ capabilities = ["read"] }}"#),
        )
        .await
        .expect("policy");
    server
        .write_policy(
            "kv-suite-write",
            &format!(r#"path "{data_path}" {{ capabilities = ["create", "update"] }}"#),
        )
        .await
        .expect("policy");
    let reader = server
        .token_with_policies(&["kv-suite-read"])
        .await
        .expect("token");
    let writer = server
        .token_with_policies(&["kv-suite-write"])
        .await
        .expect("token");
    let reader = client_with(server, reader.expose().to_owned()).await;
    let writer = client_with(server, writer.expose().to_owned()).await;

    let refused = reader
        .write_kv(&path, &data, KvCas::At(version(1)))
        .await
        .expect_err("a reader cannot write");
    assert!(
        matches!(refused, SecretError::Api { status: 403, .. }),
        "{refused:?}"
    );
    let refused = writer
        .read_kv_fields(&path, version(1), &["k"])
        .await
        .expect_err("a writer cannot read");
    assert!(
        matches!(refused, SecretError::Api { status: 403, .. }),
        "{refused:?}"
    );
    writer
        .write_kv(&path, &data, KvCas::At(version(1)))
        .await
        .expect("a writer writes");
    assert!(
        reader
            .read_kv_fields(&path, version(2), &["k"])
            .await
            .is_ok()
    );
}
