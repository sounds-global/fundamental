//! Compile-fail guard: transactions are gated at the type level per backend.
//!
//! `CqlPool` does not implement [`fundamental_db::Transactional`], so `pool.begin()` does not
//! compile.

#[test]
fn cql_pool_is_not_transactional() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_begin.rs");
}

/// The CQL seam's guard, pinned by the compiler rather than by a doc comment.
///
/// `Cql::new` takes `&'static str` and `cql!` splices only a `CqlIdent`.
#[test]
fn statement_text_cannot_be_built_from_unchecked_strings() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/no_formatted_statement.rs");
    t.compile_fail("tests/compile_fail/no_unvalidated_ident.rs");
}

/// A composite partition cannot be addressed by half of it.
///
/// `partition_key` is bounded on `SinglePartitionModel`, and `partition` takes the model's own
/// `PartitionKey` tuple.
#[test]
fn a_composite_partition_cannot_be_addressed_by_half() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/no_half_partition_key.rs");
    t.compile_fail("tests/compile_fail/no_partition_key_of_the_wrong_arity.rs");
}
