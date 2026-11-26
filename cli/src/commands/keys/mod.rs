//! `fun keys`: the key files a project's configuration reads its keyrings from.

mod input;
mod mint;
mod run;

pub(crate) use input::KeysArgs;
pub(crate) use run::run;
