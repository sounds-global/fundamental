//! Compile-fail guards; each `.stderr` snapshot pins the diagnostic.

#[test]
fn subscribing_to_an_unmounted_message_does_not_compile() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/unmounted_subscribe.rs");
}

#[test]
fn invalid_message_set_shapes_do_not_compile() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/message_set_invalid.rs");
}

#[test]
fn dotted_entity_names_do_not_compile() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/entity_name_with_dot.rs");
}

#[test]
fn a_worker_naming_a_plain_function_does_not_compile() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/unsubscribed_worker.rs");
}
