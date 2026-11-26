//! A table name is checked as the database resolves it — unquoted names fold to lower case — so
//! `Fundamental_inbox` cannot reach the inbox's ledger either.

use fundamental_macros::Model;

#[derive(Clone, Debug, Model)]
#[model(sql, table = "Fundamental_inbox")]
struct Ledger {
    #[model(id)]
    effect: String,
}

fn main() {}
