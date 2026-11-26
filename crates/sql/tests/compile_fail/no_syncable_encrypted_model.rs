//! A `#[model(syncable)]` model must not carry an encrypted `Secret` field.
//!
//! Every encrypted value is sealed to its row by a service holding the key. A device syncing the
//! row holds no key, so it could only ever hand back an envelope it was given — and accepting one
//! from a device lets a replayed envelope in under a row it was never sealed to.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_core::secrets::SecretString;
use fundamental_macros::Model;

#[derive(Model)]
#[model(sql, offline, syncable)]
struct Note {
    #[model(id)]
    id: uuid::Uuid,
    #[model(encrypted = "pii")]
    body: SecretString,
}

fn main() {}
