//! `fun local`: every project develops at its real names, on `.test`, over HTTPS.

mod input;
mod run;

pub(crate) use input::LocalArgs;
pub(crate) use run::run;
