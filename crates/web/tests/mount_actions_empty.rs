//! An empty `mount_actions! {}` compiles, with no unused-import warning from the module glob.
#![cfg(all(feature = "ssr", not(target_arch = "wasm32")))]
#![deny(warnings)]

use fundamental_web::Router;

fundamental_macros::mount_actions! {}

#[test]
fn an_empty_mount_compiles_and_mounts_nothing() {
    let router: Router<()> = register_actions(Router::new()).expect("an empty mount succeeds");
    assert_eq!(router.len(), 0);
}
