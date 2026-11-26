//! A panic's message never reaches a process's output; its location does.
//!
//! The standard hook writes from the panicking thread, so each case runs this binary as a child.

use std::env;
use std::process::Command;

/// Set on the child: which hook to panic under, `std` or `framework`.
const CHILD: &str = "FUNDAMENTAL_PANIC_HOOK_CHILD";

const VALUE: &str = "jan@example.nl";

/// The child's half; a no-op unless spawned as a child below.
#[test]
fn child_panics_on_a_value() {
    let Ok(hook) = env::var(CHILD) else {
        return;
    };
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();
    if hook == "framework" {
        fundamental_observability::install_panic_hook();
    }
    let panicked = std::thread::spawn(|| {
        let value = String::from(VALUE);
        panic!("could not handle {value}")
    })
    .join();
    assert!(panicked.is_err(), "the child thread panics");
}

fn output_under(hook: &str) -> String {
    let binary = env::current_exe().expect("the test binary knows its path");
    let output = Command::new(binary)
        .args(["--exact", "child_panics_on_a_value", "--nocapture"])
        .env(CHILD, hook)
        .output()
        .expect("the child runs");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The baseline: without it the next test could pass because the child never panicked.
#[test]
fn the_standard_hook_prints_the_message() {
    let output = output_under("std");
    assert!(
        output.contains(VALUE),
        "the standard hook quotes it:\n{output}"
    );
}

#[test]
fn the_framework_hook_logs_the_location_and_never_the_message() {
    let output = output_under("framework");
    assert!(
        output.contains("a thread panicked"),
        "the panic is reported:\n{output}"
    );
    assert!(
        output.contains("a_panic_message_never_reaches_the_output.rs"),
        "with the file it happened in:\n{output}"
    );
    assert!(!output.contains(VALUE), "and never its message:\n{output}");
}
