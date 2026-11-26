//! Two fields cannot both say they `was` the same column.
//!
//! One old column can have become only one new one: two renames from it would write a migration
//! whose second `RENAME COLUMN` fails, so the derive refuses it where it was written.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_macros::Model;

#[derive(Clone, Model)]
#[model(sql, table = "articles")]
struct Article {
    #[model(id, shard = hash)]
    id: uuid::Uuid,
    #[model(was = "headline")]
    title: String,
    #[model(was = "headline")]
    heading: String,
}

fn main() {}
