use crate::hydrate::hydrate;

/// Sets up the page's island hydration context, then runs `boot` under its root owner.
///
/// Context `boot` provides is visible to every island, before the first one hydrates.
pub fn hydrate_with(boot: impl FnOnce()) {
    hydrate();
    boot();
}
