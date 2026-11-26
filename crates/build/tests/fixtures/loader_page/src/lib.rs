//! A project whose pages use every part of the page pipeline, built by `fixture_builds.rs`.

mod overview;
mod tab;
mod team_frame;

pub use overview::Overview;
pub use tab::Tab;
pub use team_frame::TeamFrame;

#[cfg(feature = "ssr")]
mod app_state;
#[cfg(feature = "ssr")]
pub mod team;

#[cfg(feature = "ssr")]
pub use app_state::AppState;

/// The state every page of this project is served over.
#[cfg(feature = "ssr")]
pub type PageState = AppState;

#[cfg(feature = "ssr")]
include!(concat!(env!("OUT_DIR"), "/pages.rs"));

#[cfg(feature = "hydrate")]
include!(concat!(env!("OUT_DIR"), "/hydrate.rs"));
