//! A device key in a TPM, proved against `swtpm`, which is required rather than skipped.
#![cfg(all(feature = "client-tpm", target_os = "linux"))]

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use fundamental_keys::{
    DeviceKey as _, KeyCustody, MemorySecretStore, SecretStore as _, TpmDeviceKey,
};
use p256::ecdsa::signature::Verifier as _;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey as _;

struct Simulator {
    child: Child,
    state: PathBuf,
    port: u16,
}

/// How many times a simulator starts on fresh ports: a free port can be taken before `swtpm`
/// binds it.
const ATTEMPTS: usize = 5;

/// Two adjacent free loopback ports: the server's and, one above, the control channel's.
fn free_ports() -> (u16, u16) {
    loop {
        let server = TcpListener::bind(("127.0.0.1", 0)).expect("a free loopback port");
        let port = server.local_addr().expect("its address").port();
        if let Some(ctrl) = port.checked_add(1)
            && TcpListener::bind(("127.0.0.1", ctrl)).is_ok()
        {
            return (port, ctrl);
        }
    }
}

impl Simulator {
    fn start() -> Self {
        for _ in 0..ATTEMPTS {
            if let Some(simulator) = Self::try_start() {
                return simulator;
            }
        }
        panic!("swtpm did not listen after {ATTEMPTS} attempts on fresh ports")
    }

    fn try_start() -> Option<Self> {
        let (port, ctrl) = free_ports();
        let state =
            std::env::temp_dir().join(format!("fundamental-swtpm-{}-{port}", std::process::id()));
        std::fs::create_dir_all(&state).expect("a state directory");
        let child = Command::new("swtpm")
            .args(["socket", "--tpm2", "--tpmstate"])
            .arg(format!("dir={}", state.display()))
            .args(["--server", &format!("type=tcp,port={port}")])
            .args(["--ctrl", &format!("type=tcp,port={ctrl}")])
            .args(["--flags", "not-need-init,startup-clear"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| {
                panic!("swtpm is required to prove the TPM backend (dnf install swtpm / apt install swtpm): {error}")
            });
        let mut simulator = Self { child, state, port };
        let deadline = Instant::now() + Duration::from_secs(10);
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            let exited = simulator
                .child
                .try_wait()
                .is_ok_and(|status| status.is_some());
            if exited || Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(simulator)
    }

    fn tcti(&self) -> String {
        format!("swtpm:host=127.0.0.1,port={}", self.port)
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.state);
    }
}

fn verifies(key: &dyn fundamental_keys::DeviceKey, message: &[u8], signature: &[u8]) -> bool {
    let verifying = VerifyingKey::from_public_key_der(key.public_key_der()).expect("P-256 SPKI");
    let signature = Signature::from_der(signature).expect("DER ECDSA");
    verifying.verify(message, &signature).is_ok()
}

#[test]
fn a_key_the_tpm_makes_signs_what_its_public_half_verifies() {
    let tpm = Simulator::start();
    let store = MemorySecretStore::new();
    let key = TpmDeviceKey::open_or_create(&tpm.tcti(), &store, "device").expect("made in the TPM");
    assert_eq!(key.custody(), KeyCustody::Tpm);
    let signature = key.sign(b"a handshake").expect("signed");
    assert!(verifies(&key, b"a handshake", &signature));
    assert!(
        !verifies(&key, b"another message", &signature),
        "a signature over one message only"
    );
    let kept = store
        .load("device")
        .expect("read")
        .expect("the TPM's blob is kept");
    assert!(
        kept.expose().starts_with("tpm2-p256:v1:"),
        "a wrapped blob, never a private key"
    );
}

#[test]
fn the_same_key_opens_again_from_what_the_store_keeps() {
    let tpm = Simulator::start();
    let store = MemorySecretStore::new();
    let first = TpmDeviceKey::open_or_create(&tpm.tcti(), &store, "device").expect("made");
    let again = TpmDeviceKey::open_or_create(&tpm.tcti(), &store, "device").expect("opened");
    assert_eq!(
        first.public_key_der(),
        again.public_key_der(),
        "one device, one key"
    );
    let signature = again.sign(b"renewal").expect("signed");
    assert!(verifies(&first, b"renewal", &signature));
}

/// A stored key from another TPM is refused, never replaced.
#[test]
fn another_tpm_refuses_the_key_rather_than_making_a_new_one() {
    let store = MemorySecretStore::new();
    {
        let first = Simulator::start();
        TpmDeviceKey::open_or_create(&first.tcti(), &store, "device").expect("made");
    }
    let kept = store
        .load("device")
        .expect("read")
        .expect("kept")
        .expose()
        .to_owned();
    let other = Simulator::start();
    let refused = TpmDeviceKey::open_or_create(&other.tcti(), &store, "device")
        .expect_err("another TPM cannot load it");
    assert!(
        matches!(refused, fundamental_keys::KeyError::Custody(_)),
        "{refused:?}"
    );
    assert_eq!(
        store.load("device").expect("read").expect("kept").expose(),
        &kept,
        "left as it was"
    );
}
