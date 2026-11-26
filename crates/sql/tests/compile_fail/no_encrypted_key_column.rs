//! An encrypted `Secret` field must not be a key column.
//!
//! A secret is sealed with a fresh nonce on every write, so a key built from one could never
//! address the row it names — and a binding that includes it would bind nothing stable.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_core::secrets::SecretString;
use fundamental_macros::Model;

#[derive(Model)]
#[model(sql)]
struct Account {
    #[model(id, encrypted = "pii")]
    token: SecretString,
    email: String,
}

fn main() {}
