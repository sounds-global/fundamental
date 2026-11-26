//! `fun messages`: a project's MessageFormat 2 messages, checked per language.

mod check;
mod input;

pub(crate) use check::run;
pub(crate) use input::MessagesArgs;
