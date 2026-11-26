//! `#[model(unbound)]` must not name an encrypted column.
//!
//! `unbound` exempts a bookkeeping column from the row binding every encrypted value is sealed
//! to. An encrypted column is what is sealed, not what it is sealed to, so the attribute on one
//! is a misunderstanding worth refusing rather than ignoring.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_core::secrets::SecretString;
use fundamental_macros::Model;

#[derive(Model)]
#[model(sql)]
struct Account {
    #[model(id)]
    id: uuid::Uuid,
    #[model(unbound, encrypted = "pii")]
    token: SecretString,
}

fn main() {}
