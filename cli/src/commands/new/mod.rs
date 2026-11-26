//! `fun new`: scaffold a new Fundamental project.

mod generate;
mod input;
mod run;

pub(crate) use generate::{
    generate_app_crate, generate_native_crate, islands_cargo_toml, islands_lib_rs, ssr_gate,
    surface_items, surface_line,
};
pub(crate) use input::NewArgs;
pub(crate) use run::run;
