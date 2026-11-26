//! Round-trip a value through field-level encryption.
//!
//! Run with `cargo run --example encrypt_field -p fundamental_encryption --all-features`.

use fundamental_encryption::{Encrypted, KeyManager, RowBinding};

fn main() {
    let key_bytes = [0x42u8; 32];

    let manager = KeyManager::builder()
        .with_local_key("pii", &key_bytes)
        .build()
        .expect("failed to build KeyManager");

    let plaintext = "alice@example.com";
    println!("plaintext: {plaintext}");

    // Sealed to its row: table, column and every other column by name.
    let parts = vec![("name", Some(b"\x01alice".to_vec()))];
    let row = RowBinding::new("people", "email", &parts);

    let encrypted = manager
        .encrypt_string(plaintext, "pii", &row)
        .expect("encryption failed");

    // Debug shows metadata and ciphertext length, never the plaintext.
    println!("encrypted: {encrypted:?}");
    println!(
        "  key_id={}, key_version={}, ciphertext_len={}",
        encrypted.key_id(),
        encrypted.key_version(),
        encrypted.ciphertext().len()
    );

    assert_ne!(encrypted.ciphertext(), plaintext.as_bytes());

    let decrypted = manager
        .decrypt_string(&encrypted, &row)
        .expect("decryption failed");
    println!("decrypted: {decrypted}");

    assert_eq!(decrypted, plaintext, "round-trip mismatch");
    println!("round-trip succeeded: plaintext == decrypted");
}
