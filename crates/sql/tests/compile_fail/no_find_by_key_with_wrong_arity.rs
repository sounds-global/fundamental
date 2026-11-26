//! `find_by_key` must not accept a key tuple of the wrong width.
//!
//! The other two pins in this directory cover the *absence* of `find`/`delete` on a
//! composite-key model. This covers the presence of `find_by_key`: it exists, and the thing
//! that must not compile is calling it with a key that is the wrong shape.
//!
//! A `&[&str]` parameter would have accepted every width and coerced every column to text.
//! `M::KeyValues` is a tuple of the key columns' own field types, so both the arity and the
//! element types are the compiler's — which is what makes a prefix-less scan and a
//! text-vs-`UUID` comparison unreachable rather than merely discouraged.
//!
//! Expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_sql::SqlModelQuery;
use fundamental_macros::Model;

#[derive(Model)]
#[model(sql)]
struct HolderAccount {
    #[model(id)]
    zone: String,
    #[model(id)]
    holder_account_id: String,
    created_at: String,
}

async fn fetches_with_half_a_key(pool: &fundamental_sql::SqlPool) {
    // One element where the model declares two. The key is `(zone, holder_account_id)`.
    let _ = SqlModelQuery::<HolderAccount>::find_by_key(pool, &("eu".to_string(),)).await;
}

fn main() {}
