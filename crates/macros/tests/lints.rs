//! `lints!()` compiles the nearest `fun.toml`'s migration lint levels into the binary.

use fundamental::db::migration::Lints;

#[test]
fn the_levels_of_the_nearest_fun_toml_are_compiled_in() {
    // This workspace's `fun.toml` sets no `[migrations]`, so it holds every lint to its default.
    let lints = fundamental::db::migration::lints!().expect("the workspace's fun.toml reads");

    assert_eq!(lints, Lints::default());
}
