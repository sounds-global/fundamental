//! A retired column is one the model no longer has.
//!
//! Retiring a column the model still declares would have the next migration both keep it and drop
//! it, so the derive refuses it where it was written.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_macros::Model;

#[derive(Clone, Model)]
#[model(sql, table = "articles", retired(columns = ["title"]))]
struct Article {
    #[model(id, shard = hash)]
    id: uuid::Uuid,
    title: String,
}

fn main() {}
