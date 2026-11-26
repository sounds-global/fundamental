//! `window.__hydrateWithin(root)` hydrates a swapped-in part's islands, parents before children.
#![cfg(all(feature = "ssr", not(target_arch = "wasm32")))]

use fundamental_web::hydration_bootstrap;

#[test]
fn the_bootstrap_hydrates_the_islands_within_a_part() {
    let bootstrap = hydration_bootstrap();
    assert!(
        bootstrap.contains("window.__hydrateWithin = (root) => traverse(root, mod);"),
        "no walk for a swapped part: {bootstrap}"
    );
}

#[test]
fn a_part_is_walked_only_once_the_bundle_is_ready() {
    let bootstrap = hydration_bootstrap();
    let within = bootstrap
        .find("window.__hydrateWithin")
        .expect("the walk for a part");
    let ready = bootstrap.find("mod.hydrate();").expect("the root owner");
    assert!(
        within > ready,
        "a part could be walked before the root owner every island hydrates beneath"
    );
}
