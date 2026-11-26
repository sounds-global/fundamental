//! The ErrorOverride derive.

mod derive;
mod override_attr;

use override_attr::OverrideAttr;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match derive::error_override_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
