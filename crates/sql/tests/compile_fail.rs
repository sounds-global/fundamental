//! Compile-fail guards: half a composite key cannot address a row, and other refused shapes.

#[test]
fn a_composite_key_model_has_no_find_by_half_a_key() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_find_by_partial_key.rs");
}

#[test]
fn a_composite_key_model_has_no_delete_by_half_a_key() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_delete_by_partial_key.rs");
}

/// A slice parameter would accept a key of the wrong width.
#[test]
fn find_by_key_refuses_a_key_tuple_of_the_wrong_width() {
    trybuild::TestCases::new()
        .compile_fail("tests/compile_fail/no_find_by_key_with_wrong_arity.rs");
}

#[test]
fn a_composite_key_row_cannot_be_locked_by_half_a_key() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_lock_by_partial_key.rs");
}

/// Every read of a composite-key model must pin its leading key, or it scans every tenancy.
#[test]
fn a_composite_key_model_cannot_be_read_without_pinning_its_leading_key() {
    trybuild::TestCases::new()
        .compile_fail("tests/compile_fail/no_unpinned_read_of_a_composite_key.rs");
}

#[test]
fn a_composite_key_model_has_no_whole_table_count_or_update() {
    trybuild::TestCases::new()
        .compile_fail("tests/compile_fail/no_unpinned_count_of_a_composite_key.rs");
}

/// A defaulting `begin()` would leave the level to the deployed engine's configuration.
#[test]
fn a_transaction_cannot_be_opened_without_an_isolation_level() {
    trybuild::TestCases::new()
        .compile_fail("tests/compile_fail/no_transaction_without_an_isolation_level.rs");
}

/// YSQL ignores `SKIP LOCKED` under `SERIALIZABLE`, so the clause lives beside its isolation.
#[test]
fn skip_locked_cannot_be_named_outside_the_claim() {
    trybuild::TestCases::new()
        .compile_fail("tests/compile_fail/no_skip_locked_outside_the_claim.rs");
}

/// The `Clone` bound proves a second attempt is safe.
#[test]
fn a_transaction_body_that_cannot_be_rerun_is_refused() {
    trybuild::TestCases::new()
        .compile_fail("tests/compile_fail/no_transaction_that_cannot_be_rerun.rs");
}

/// A secret is sealed with a fresh nonce, so a key built from one could never address its row.
#[test]
fn an_encrypted_field_cannot_be_a_key_column() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_encrypted_key_column.rs");
}

#[test]
fn an_encrypted_column_cannot_be_unbound() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_unbound_encrypted_column.rs");
}

#[test]
fn a_bound_column_without_an_encoding_names_the_fix() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_unencodable_bound_column.rs");
}

#[test]
fn a_key_column_cannot_be_unbound() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_unbound_key_column.rs");
}

#[test]
fn a_syncable_model_cannot_carry_an_encrypted_field() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_syncable_encrypted_model.rs");
}

#[test]
fn an_outbox_row_must_be_sealed() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_outbox_row_without_a_seal.rs");
}

#[test]
fn an_erasable_column_needs_an_outbox() {
    trybuild::TestCases::new()
        .compile_fail("tests/compile_fail/no_erasable_column_outside_an_outbox.rs");
}

#[test]
fn an_index_key_cannot_pin_another_models_query() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_pin_by_another_models_index.rs");
}

#[test]
fn an_index_on_an_unknown_column_does_not_expand() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_index_on_an_unknown_column.rs");
}

#[test]
fn a_retired_column_is_one_the_model_no_longer_has() {
    trybuild::TestCases::new()
        .compile_fail("tests/compile_fail/no_retired_column_the_model_still_has.rs");
}

#[test]
fn was_cannot_name_another_column() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_was_naming_another_column.rs");
}

#[test]
fn two_columns_cannot_both_have_been_one() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_two_columns_that_were_one.rs");
}

#[test]
fn a_constraint_index_is_unique() {
    trybuild::TestCases::new()
        .compile_fail("tests/compile_fail/no_constraint_index_that_is_not_unique.rs");
}
