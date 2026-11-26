//! Compile-fail guards for the attribute macros.

/// An action's typed `path()` refuses a raw id, so no form can carry a UUID to its endpoint.
#[test]
fn an_action_path_refuses_a_raw_id() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/no_raw_id_in_an_action_path.rs");
}
