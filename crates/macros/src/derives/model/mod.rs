//! `#[derive(Model)]`: a struct bound to YSQL, YCQL and/or offline SQLite.

mod database_binding;
mod derive;
mod field_attrs;
mod field_info;
mod struct_attrs;

use database_binding::DatabaseBinding;
use field_attrs::FieldAttrs;
use field_info::{ColumnSpec, FieldInfo, SchemaBackend};
use struct_attrs::StructAttrs;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match derive::model_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
