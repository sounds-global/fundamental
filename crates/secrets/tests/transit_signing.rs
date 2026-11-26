//! The Transit signing client against a real OpenBao engine.
//!
//! Needs Docker, and fails, never skips, without it.

use std::num::NonZeroU32;

use fundamental_config::settings::{VaultAuth, VaultSettings};
use fundamental_core::Error as _;
use fundamental_core::secrets::InMemorySecret;
use fundamental_keys::{
    AsyncSigner, Ed25519, Es256, KeyError, KeyProtection, KmsSigner, Rs256, SignatureAlgorithm,
};
use fundamental_secrets::VaultClient;
use fundamental_secrets::transit::{
    TransitKeyName, TransitKeyRef, TransitKeyType, TransitMount, TransitSignerLimits,
    VaultTransitClient,
};
use fundamental_test_containers::container_name::OPENBAO_01_TRANSIT;
use fundamental_test_containers::openbao::OpenBaoServer;
use serde_json::json;

const MOUNT: &str = "transit-suite";

async fn engine() -> (&'static OpenBaoServer, VaultClient) {
    let server = OpenBaoServer::shared(OPENBAO_01_TRANSIT)
        .await
        .expect("an OpenBao container (is Docker running?)");
    server.enable_transit(MOUNT).await.expect("transit mounted");
    let settings = VaultSettings {
        enabled: true,
        address: server.address().to_owned(),
        auth: VaultAuth::Token {
            token: InMemorySecret::new(server.root_token().to_owned()),
        },
        ..VaultSettings::default()
    };
    let client = VaultClient::from_settings(&settings)
        .await
        .expect("client")
        .expect("enabled");
    (server, client)
}

fn fresh_name() -> TransitKeyName {
    TransitKeyName::parse(&format!("t-{}", uuid::Uuid::new_v4().simple())).expect("valid")
}

fn mount() -> TransitMount {
    TransitMount::parse(MOUNT).expect("valid")
}

#[tokio::test]
async fn ensure_signing_key_is_idempotent() {
    let (_, vault) = engine().await;
    let name = fresh_name();
    let first = vault
        .transit_ensure_signing_key(&mount(), &name, TransitKeyType::EcdsaP256)
        .await
        .expect("created");
    let second = vault
        .transit_ensure_signing_key(&mount(), &name, TransitKeyType::EcdsaP256)
        .await
        .expect("ensured again");
    assert_eq!(
        first.public_keys, second.public_keys,
        "the same key, not a second one"
    );
    assert!(!second.exportable);
    assert!(!second.allow_plaintext_backup);
}

#[tokio::test]
async fn a_transit_es256_signature_verifies_under_the_read_public_key() {
    let (_, vault) = engine().await;
    let name = fresh_name();
    vault
        .transit_ensure_signing_key(&mount(), &name, TransitKeyType::EcdsaP256)
        .await
        .expect("created");
    let client = VaultTransitClient::<Es256>::new(
        vault,
        TransitKeyRef::new(mount(), name, NonZeroU32::MIN),
        TransitSignerLimits::default(),
    );
    let signer = KmsSigner::open("tenant-k1", client).await.expect("opens");
    assert_eq!(
        signer.protection(),
        KeyProtection::Software,
        "Transit claims no hardware custody"
    );

    let message = b"entity configuration";
    let signature = signer.sign(message).await.expect("signs");
    Es256::verify(&signer.public_key(), message, &signature).expect("verifies");
}

#[tokio::test]
async fn an_ed25519_transit_signature_verifies() {
    let (_, vault) = engine().await;
    let name = fresh_name();
    vault
        .transit_ensure_signing_key(&mount(), &name, TransitKeyType::Ed25519)
        .await
        .expect("created");
    let client = VaultTransitClient::<Ed25519>::new(
        vault,
        TransitKeyRef::new(mount(), name, NonZeroU32::MIN),
        TransitSignerLimits::default(),
    );
    let signer = KmsSigner::open("witness", client).await.expect("opens");
    let signature = signer.sign(b"checkpoint").await.expect("signs");
    Ed25519::verify(&signer.public_key(), b"checkpoint", &signature).expect("verifies");
}

#[tokio::test]
async fn an_rs256_transit_signature_verifies() {
    let (_, vault) = engine().await;
    let name = fresh_name();
    let created = vault
        .transit_ensure_signing_key(&mount(), &name, TransitKeyType::Rsa2048)
        .await
        .expect("created");
    assert_eq!(created.key_type, "rsa-2048");
    let client = VaultTransitClient::<Rs256>::new(
        vault,
        TransitKeyRef::new(mount(), name, NonZeroU32::MIN),
        TransitSignerLimits::default(),
    );
    let signer = KmsSigner::open("code-host-app", client)
        .await
        .expect("opens");
    let message = b"eyJhbGciOiJSUzI1NiJ9.eyJpc3MiOiIxMjMifQ";
    let signature = signer.sign(message).await.expect("signs");
    assert_eq!(signature.as_bytes().len(), 256, "a 2048-bit modulus");
    Rs256::verify(&signer.public_key(), message, &signature).expect("verifies");
    assert!(
        Rs256::verify(&signer.public_key(), b"another message", &signature).is_err(),
        "the signature is bound to its message"
    );
}

#[tokio::test]
async fn an_exportable_rsa_transit_key_cannot_open() {
    let (server, vault) = engine().await;
    let name = fresh_name();
    let created = server
        .post(
            &format!("{MOUNT}/keys/{}", name.as_str()),
            &json!({ "type": "rsa-2048", "exportable": true }),
        )
        .await
        .expect("arranged");
    assert!(created.status().is_success());

    let client = VaultTransitClient::<Rs256>::new(
        vault,
        TransitKeyRef::new(mount(), name, NonZeroU32::MIN),
        TransitSignerLimits::default(),
    );
    let error = KmsSigner::open("code-host-app", client)
        .await
        .err()
        .expect("refused");
    assert!(matches!(error, KeyError::ExportableKeyRefused));
}

#[tokio::test]
async fn an_rsa_key_is_not_mistaken_for_an_es256_one() {
    let (_, vault) = engine().await;
    let name = fresh_name();
    vault
        .transit_ensure_signing_key(&mount(), &name, TransitKeyType::Rsa2048)
        .await
        .expect("created");
    let client = VaultTransitClient::<Es256>::new(
        vault,
        TransitKeyRef::new(mount(), name, NonZeroU32::MIN),
        TransitSignerLimits::default(),
    );
    let error = KmsSigner::open("tenant", client)
        .await
        .err()
        .expect("refused");
    assert!(matches!(error, KeyError::AlgorithmMismatch));
}

#[tokio::test]
async fn an_existing_key_of_another_type_is_refused() {
    let (_, vault) = engine().await;
    let name = fresh_name();
    vault
        .transit_ensure_signing_key(&mount(), &name, TransitKeyType::Ed25519)
        .await
        .expect("created as ed25519");
    // The engine answers this create with the existing ed25519 key.
    let answered = vault
        .transit_ensure_signing_key(&mount(), &name, TransitKeyType::EcdsaP256)
        .await
        .expect("answered");
    assert_eq!(answered.key_type, "ed25519");

    let client = VaultTransitClient::<Es256>::new(
        vault,
        TransitKeyRef::new(mount(), name, NonZeroU32::MIN),
        TransitSignerLimits::default(),
    );
    let error = KmsSigner::open("tenant", client)
        .await
        .err()
        .expect("refused");
    assert!(matches!(error, KeyError::AlgorithmMismatch));
    assert_eq!(error.error_code(), "KEYS_009");
}

#[tokio::test]
async fn an_exportable_transit_key_cannot_open() {
    let (server, vault) = engine().await;
    let name = fresh_name();
    let created = server
        .post(
            &format!("{MOUNT}/keys/{}", name.as_str()),
            &json!({ "type": "ecdsa-p256", "exportable": true }),
        )
        .await
        .expect("arranged");
    assert!(created.status().is_success());

    let client = VaultTransitClient::<Es256>::new(
        vault,
        TransitKeyRef::new(mount(), name, NonZeroU32::MIN),
        TransitSignerLimits::default(),
    );
    let error = KmsSigner::open("tenant", client)
        .await
        .err()
        .expect("refused");
    assert!(matches!(error, KeyError::ExportableKeyRefused));
}

#[tokio::test]
async fn sign_pins_the_enrolled_key_version() {
    let (server, vault) = engine().await;
    let name = fresh_name();
    vault
        .transit_ensure_signing_key(&mount(), &name, TransitKeyType::EcdsaP256)
        .await
        .expect("created");
    let client = VaultTransitClient::<Es256>::new(
        vault.clone(),
        TransitKeyRef::new(mount(), name.clone(), NonZeroU32::MIN),
        TransitSignerLimits::default(),
    );
    let signer = KmsSigner::open("tenant-k1", client).await.expect("opens");

    let rotated = server
        .post(
            &format!("{MOUNT}/keys/{}/rotate", name.as_str()),
            &json!({}),
        )
        .await
        .expect("rotated out of band");
    assert!(rotated.status().is_success());
    let after = vault
        .transit_read_signing_key(&mount(), &name)
        .await
        .expect("read");
    assert_eq!(after.latest_version, 2);

    let signature = signer
        .sign(b"after rotation")
        .await
        .expect("still signs with v1");
    Es256::verify(&signer.public_key(), b"after rotation", &signature).expect("verifies under v1");
}

#[tokio::test]
async fn a_missing_key_is_a_refusal_not_an_outage() {
    let (_, vault) = engine().await;
    let client = VaultTransitClient::<Es256>::new(
        vault,
        TransitKeyRef::new(mount(), fresh_name(), NonZeroU32::MIN),
        TransitSignerLimits::default(),
    );
    let error = KmsSigner::open("tenant", client)
        .await
        .err()
        .expect("refused");
    assert!(matches!(error, KeyError::Custody(_)), "{error:?}");
    assert!(!error.is_retryable());
}
