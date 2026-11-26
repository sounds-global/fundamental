//! A model with encrypted fields must not bind a column type that has no canonical encoding.
//!
//! Every non-secret column is bound into the model's encrypted values, and a type without a
//! `RowBound` encoding could bind differently on write and on read. The diagnostic names both
//! ways out: implement `RowBound`, or mark the column `#[model(unbound)]`.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_core::secrets::SecretString;
use fundamental_macros::Model;

#[derive(Model)]
#[model(sql)]
struct Reading {
    #[model(id)]
    id: uuid::Uuid,
    temperature: f64,
    #[model(encrypted = "pii")]
    note: SecretString,
}

fn main() {}
