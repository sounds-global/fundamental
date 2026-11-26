//! The wasm build seam: compile a web app's browser bundle behind one place.

mod islands_crate;
mod locked_bindgen;
mod wasm_artifacts;
mod wasm_build;

pub use islands_crate::IslandsCrate;
pub use locked_bindgen::locked_bindgen_cli;
pub use wasm_artifacts::WasmArtifacts;
pub use wasm_build::{WasmBuild, bindgen_cli_version};
