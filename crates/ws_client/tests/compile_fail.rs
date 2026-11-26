//! The removed `reconnect_max_attempts` cannot be set; the `.stderr` snapshot pins the message.

#![cfg(not(target_arch = "wasm32"))]

#[test]
fn setting_reconnect_max_attempts_does_not_compile() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/reconnect_max_attempts.rs");
}
