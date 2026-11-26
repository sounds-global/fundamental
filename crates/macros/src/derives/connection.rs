//! `#[derive(Connection)]`: a fieldless unit struct as a typed database connection name.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

use crate::derives::case::to_snake_case;
use crate::derives::core_path;
use crate::derives::entity_name::validate_entity_name;

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "Connection cannot be derived for a generic type; a connection is a single fixed marker",
        ));
    }

    match &input.data {
        Data::Struct(data) => {
            if !matches!(data.fields, Fields::Unit) {
                return Err(syn::Error::new_spanned(
                    name,
                    "Connection can only be derived for a unit struct (e.g. `struct Reports;`); \
                     a connection marker carries no data",
                ));
            }
        }
        Data::Enum(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "Connection can only be derived for a unit struct, not an enum",
            ));
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "Connection can only be derived for a unit struct, not a union",
            ));
        }
    }

    let name_override = parse_connection_name(input)?;
    let connection_name = match &name_override {
        Some(literal) => literal.value(),
        None => to_snake_case(&name.to_string()),
    };
    let name_span = name_override
        .as_ref()
        .map_or_else(|| name.span(), LitStr::span);
    validate_entity_name(&connection_name, name_span, "Connection")?;

    let core = core_path();

    Ok(quote! {
        impl #core::model::Connection for #name {
            const NAME: &'static str = #connection_name;
        }
    })
}

/// An optional `#[connection(name = "…")]`, kept as a literal for its span.
fn parse_connection_name(input: &DeriveInput) -> syn::Result<Option<LitStr>> {
    let mut name = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("connection") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                name = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("unknown `#[connection(...)]` attribute; valid: name = \"…\""))
            }
        })?;
    }
    Ok(name)
}
