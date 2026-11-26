use leptos::mount::hydrate_islands;

/// Sets up the reactive hydration context for the current page's islands; the bootstrap walks the DOM.
pub fn hydrate() {
    hydrate_islands();
}
