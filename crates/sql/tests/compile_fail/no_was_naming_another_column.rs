//! `was` names a column's name before, which no other column of the model may have.
//!
//! A rename from a name the model still uses would make the next migration rename a column away
//! from under the field that keeps it, so the derive refuses it where it was written.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_macros::Model;

#[derive(Clone, Model)]
#[model(sql, table = "articles")]
struct Article {
    #[model(id, shard = hash)]
    id: uuid::Uuid,
    #[model(was = "id")]
    title: String,
}

fn main() {}
