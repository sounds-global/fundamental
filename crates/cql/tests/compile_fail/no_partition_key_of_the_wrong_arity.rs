//! The typed partition binder takes the model's whole key, or it does not compile.
//!
//! `CqlQuery::partition` takes `M::PartitionKey` — the tuple of the partition columns'
//! field types, in declaration order — so a call that supplies one value where the model
//! declares two names both types and fails. This is the half of the partition rule made
//! *impossible* rather than merely refused: the runtime check in `build_where_clause`
//! stands behind the untyped `partition_key_composite` door only.
//!
//! This file is expected to fail to compile; the paired `.stderr` snapshot pins the error.

use fundamental_cql::CqlPool;

#[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
#[model(cql, table = "standing_accesses")]
struct StandingAccess {
    #[model(partition_key)]
    zone: String,
    #[model(partition_key)]
    subject_ppid: String,
    #[model(clustering_key)]
    grant_id: String,
    state: String,
}

fn addresses_half_a_partition(pool: &CqlPool, zone: String) {
    // `StandingAccess::PartitionKey` is `(String, String)`; a bare `String` is not it.
    let _ = pool.query::<StandingAccess>().partition(zone);
}

fn main() {}
