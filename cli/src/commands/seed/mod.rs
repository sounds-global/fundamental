//! `fun seed`: run a project's database seeds.

mod input;
mod run;

pub(crate) use input::SeedArgs;
pub(crate) use run::run;
