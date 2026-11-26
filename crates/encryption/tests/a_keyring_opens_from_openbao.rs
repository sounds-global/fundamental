//! A keyring kept in OpenBao opens; anything missing or malformed refuses. Needs Docker.

use fundamental_config::settings::{VaultAuth, VaultSettings};
use fundamental_core::Error as _;
use fundamental_core::secrets::InMemorySecret;
use fundamental_encryption::{KeyPurpose, Keyring, KeyringError};
use fundamental_secrets::{KvCas, KvData, KvPath, VaultClient};
use fundamental_test_containers::container_name::OPENBAO_04_KV;
use fundamental_test_containers::openbao::OpenBaoServer;

const MOUNT: &str = "keyring-suite";

struct SuiteKey;

impl KeyPurpose for SuiteKey {
    const NAME: &'static str = "suite.key";
    const MAX_PLAINTEXT_LEN: usize = 64;
}

fn material(purpose: &str, version: u16, byte: &str) -> String {
    format!("{purpose}:{version}:{}", byte.repeat(32))
}

async fn vault() -> VaultClient {
    let server = OpenBaoServer::shared(OPENBAO_04_KV)
        .await
        .expect("an OpenBao container (is Docker running?)");
    server.enable_kv(MOUNT).await.expect("kv mounted");
    let settings = VaultSettings {
        enabled: true,
        address: server.address().to_owned(),
        auth: VaultAuth::Token {
            token: InMemorySecret::new(server.root_token().to_owned()),
        },
        ..VaultSettings::default()
    };
    VaultClient::from_settings(&settings)
        .await
        .expect("client")
        .expect("enabled")
}

async fn kept(vault: &VaultClient, fields: &[(&str, &str)]) -> KvPath {
    let path = KvPath::new(
        MOUNT,
        &format!("keyrings/{}", uuid::Uuid::new_v4().simple()),
    )
    .expect("a path");
    vault
        .write_kv(&path, &KvData::new(fields).expect("fields"), KvCas::Absent)
        .await
        .expect("written");
    path
}

#[tokio::test]
async fn every_version_up_to_the_current_one_opens() {
    let vault = vault().await;
    let old = material("suite.key", 1, "1a");
    let new = material("suite.key", 2, "2b");
    let path = kept(
        &vault,
        &[("current", "2"), ("material_1", &old), ("material_2", &new)],
    )
    .await;
    let keyring = Keyring::<SuiteKey>::from_vault(&vault, &path)
        .await
        .expect("opens");
    assert_eq!(keyring.current_version().get(), 2);
    let sealed_before =
        Keyring::<SuiteKey>::from_settings(&fundamental_encryption::KeyringSettings {
            current: 1,
            versions: vec![fundamental_encryption::KeyringKeySettings {
                version: 1,
                material: fundamental_core::secrets::InMemorySecretString::new(old),
            }],
        })
        .expect("the old keyring")
        .seal(b"kept", &[b"ctx"])
        .expect("sealed");
    assert_eq!(
        keyring
            .open(&sealed_before, &[b"ctx"])
            .expect("opens")
            .as_slice(),
        b"kept",
        "a token sealed under an older version still opens"
    );
}

#[tokio::test]
async fn a_keyring_openbao_does_not_hold_is_refused() {
    let vault = vault().await;
    let path = KvPath::new(MOUNT, "keyrings/never-written").expect("a path");
    let refused = Keyring::<SuiteKey>::from_vault(&vault, &path)
        .await
        .expect_err("refused");
    assert!(
        matches!(refused, KeyringError::Unreadable { .. }),
        "{refused}"
    );
    assert_eq!(refused.error_code(), "KEYRING_007");
    assert_eq!(refused.http_status_code(), 500);
}

#[tokio::test]
async fn a_current_that_is_no_version_is_refused() {
    let vault = vault().await;
    let one = material("suite.key", 1, "1a");
    for current in ["two", "0", ""] {
        let path = kept(&vault, &[("current", current), ("material_1", &one)]).await;
        let refused = Keyring::<SuiteKey>::from_vault(&vault, &path)
            .await
            .expect_err("refused");
        assert_eq!(refused.error_code(), "KEYRING_007", "current {current:?}");
    }
}

#[tokio::test]
async fn a_missing_version_is_refused() {
    let vault = vault().await;
    let two = material("suite.key", 2, "2b");
    let path = kept(&vault, &[("current", "2"), ("material_2", &two)]).await;
    let refused = Keyring::<SuiteKey>::from_vault(&vault, &path)
        .await
        .expect_err("refused");
    assert_eq!(refused.error_code(), "KEYRING_007");
    assert!(refused.to_string().contains("material_1"), "{refused}");
}

#[tokio::test]
async fn material_for_another_purpose_is_refused_as_the_settings_are() {
    let vault = vault().await;
    let other = material("other.key", 1, "1a");
    let path = kept(&vault, &[("current", "1"), ("material_1", &other)]).await;
    let refused = Keyring::<SuiteKey>::from_vault(&vault, &path)
        .await
        .expect_err("refused");
    assert_eq!(refused.error_code(), "KEYRING_001");
}
