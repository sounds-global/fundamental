//! `#[derive(Consumer)]` — turn a unit struct into a typed JetStream consumer handle.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

use crate::derives::entity_name;

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match entity_name::expand(&input, "Consumer", "consumer") {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
