//! A runtime-built `String` must not become a [`Cql`] statement by accident.
//!
//! `Cql::new` takes `&'static str`, so the ordinary constructor cannot be handed
//! a `format!`-ed string — which is the whole point of the type. Building a
//! statement from arbitrary text is still possible, but only through the
//! deliberately-named `Cql::interpolated`, so those sites can be counted. This
//! file is expected to fail to compile; the paired `.stderr` snapshot pins it.

use fundamental_cql::Cql;

fn attempts_interpolation(table: &str) -> Cql {
    // The injection shape: caller-controlled text spliced into a statement.
    Cql::new(&format!("SELECT * FROM {table}"))
}

fn main() {}
