//! The hot paths the instruction and wall-clock benches measure: each a setup (`new`) and the
//! per-request call a harness times or counts.

pub mod action;
pub mod config;
pub mod mf2;
pub mod migration;
pub mod query;
pub mod render;
pub mod sync;

mod ready;
