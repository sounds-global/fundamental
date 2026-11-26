//! `fun generate`: scaffold domain code into an existing project.

mod action;
mod action_template;
mod field;
mod input;
mod model;
mod model_name;
mod model_template;
mod run;
mod slice;
mod slice_template;

pub(crate) use input::{GenerateActionArgs, GenerateArgs, GenerateModelArgs, GenerateSliceArgs};
pub(crate) use run::run;

use field::Field;
use input::GenerateCommand;
use model_name::ModelName;
