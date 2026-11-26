//! [`Hydration`], what an app's browser build hydrates and how it starts.

/// What an app's browser build hydrates, and how it starts before any island does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Hydration {
    /// Every page's islands, started by the plain `fundamental::web::hydrate()`.
    #[default]
    Plain,
    /// Every page's islands, started by `hydrate_with(|| crate::live_boot())`, so app-wide
    /// context reaches every island.
    Live,
    /// No page: a site that ships no script; the entry still compiles and hydrates nothing.
    NoPages,
}
