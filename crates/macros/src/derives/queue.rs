//! `#[derive(Queue)]` — turn a unit struct into a typed NATS queue group.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

use crate::derives::entity_name;

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match entity_name::expand(&input, "Queue", "queue") {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
