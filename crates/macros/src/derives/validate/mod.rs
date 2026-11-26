//! The Validate derive.

mod args;
mod derive;
mod generate;
mod validation;

use args::*;
use generate::ValidationGenerate;
use validation::Validation;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match derive::validate_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
