//! Compile-time guards on the inbox: a handler cannot write outside the transaction its inbox
//! row is in, and a JetStream consumer cannot forget to choose.

#[test]
fn every_way_out_of_the_transaction_does_not_compile() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/*.rs");
}

#[test]
fn the_inbox_shape_compiles() {
    trybuild::TestCases::new().pass("tests/compile_pass/*.rs");
}
