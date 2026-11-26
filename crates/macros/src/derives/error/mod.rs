//! The Error derive.

mod derive;
mod error_attr;
mod field_analysis;
mod from_field_info;
mod unauthenticated;
mod variant_info;

use crate::derives::template::Interpolate;
use error_attr::ErrorAttr;
use field_analysis::FieldAnalysis;
use from_field_info::FromFieldInfo;
use unauthenticated::generate_unauthenticated_impl;
use variant_info::VariantInfo;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match derive::error_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
