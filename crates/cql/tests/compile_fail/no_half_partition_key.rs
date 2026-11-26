//! A composite partition cannot be addressed by one value.
//!
//! `partition_key` is bounded on `SinglePartitionModel`, which `#[derive(Model)]` emits
//! only for a one-column partition — so handing it a single value for a two-column
//! partition does not resolve. Before the bound, the typestate advanced on the first value
//! whatever the model's arity and the builder emitted one `= ?` per partition *column*,
//! producing a statement whose markers and bound values disagreed at run time.
//!
//! This file is expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_cql::{CqlValue, CqlPool};

#[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
#[model(cql, table = "vault_rows")]
struct VaultRow {
    #[model(partition_key)]
    zone: String,
    #[model(partition_key)]
    owner_id: String,
    #[model(clustering_key)]
    row_id: String,
    ciphertext: String,
}

fn addresses_half_a_partition(pool: &CqlPool, zone: String) {
    // `VaultRow: !SinglePartitionModel` — its partition is `(zone, owner_id)`.
    let _ = pool.query::<VaultRow>().partition_key(CqlValue::Text(zone));
}

fn main() {}
