//! Compile-time guards for generated client APIs.

#![cfg(all(feature = "ssr", not(target_arch = "wasm32")))]

#[test]
fn an_action_cannot_be_used_with_a_mismatched_reactive_role() {
    trybuild::TestCases::new().compile_fail("tests/ui/action_role_must_match_query.rs");
}

#[test]
fn a_local_query_cannot_hold_the_page() {
    trybuild::TestCases::new().compile_fail("tests/ui/a_local_query_cannot_block.rs");
}
