//! `#[model(unbound)]` must not name a key column.
//!
//! `unbound` exempts a bookkeeping column from the row binding. A key column is what addresses the
//! row every encrypted value is sealed to, so leaving it out would let a ciphertext open under
//! another key.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_core::secrets::SecretString;
use fundamental_macros::Model;

#[derive(Model)]
#[model(sql)]
struct Account {
    #[model(id, unbound)]
    id: uuid::Uuid,
    #[model(encrypted = "pii")]
    token: SecretString,
}

fn main() {}
