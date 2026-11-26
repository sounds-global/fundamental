//! Project generation for `fun new`: a [`ProjectPlan`] written as a cargo workspace.

mod app_templates;
mod generator;
mod islands_templates;
mod native_templates;
mod readability;
mod templates;

pub(crate) use crate::commands::scaffold::ProjectPlan;
pub(crate) use app_templates::{ssr_gate, surface_items, surface_line};
pub(crate) use generator::{generate, generate_app_crate, generate_native_crate};
pub(crate) use islands_templates::{islands_cargo_toml, islands_lib_rs};
