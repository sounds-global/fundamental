//! Compile-fail guards for a closed value said through its messages.

#[test]
fn a_variant_s_fields_follow_its_message_s_inputs() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/fields_out_of_order.rs");
}
