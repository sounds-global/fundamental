//! An index a `CONSTRAINT … UNIQUE` made is unique.
//!
//! `constraint` says how a hand-written file made a unique index, so it is refused on an index that
//! does not say `unique`.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_macros::Model;

#[derive(Clone, Model)]
#[model(sql, table = "customers", index(name = "customers_code", columns = ["code hash"], constraint))]
struct Customer {
    #[model(id, shard = hash)]
    id: uuid::Uuid,
    code: String,
}

fn main() {}
