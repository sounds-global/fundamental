//! `cql!` must interpolate only a validated [`CqlIdent`], never a bare string.
//!
//! CQL cannot bind an identifier, so a keyspace or table name that varies at
//! runtime reaches the statement as text. The macro expands to
//! `CqlIdent::as_cql(&arg)`, so handing it a `&str` — the value that has *not*
//! been through the identifier guard — fails to compile. This file is expected
//! to fail; the paired `.stderr` snapshot pins it.

use fundamental_cql::{Cql, cql};

fn attempts_unvalidated_name(table: &str) -> Cql {
    cql!("SELECT id FROM {}", table)
}

fn main() {}
