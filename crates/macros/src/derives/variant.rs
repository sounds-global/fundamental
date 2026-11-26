//! `#[derive(Variant)]`: a fieldless enum as a typed axis of variation.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

use crate::derives::case::to_snake_case;
use crate::derives::core_path;

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "Variant cannot be derived for a generic enum; an axis of variation is a fixed set of tags",
        ));
    }

    let data = match &input.data {
        Data::Enum(data) => data,
        Data::Struct(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "Variant can only be derived for an enum, not a struct",
            ));
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "Variant can only be derived for an enum, not a union",
            ));
        }
    };

    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "Variant requires an enum with at least one variant",
        ));
    }

    let key = parse_enum_key(input)?.unwrap_or_else(|| to_snake_case(&name.to_string()));

    let mut idents = Vec::with_capacity(data.variants.len());
    let mut values = Vec::with_capacity(data.variants.len());
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "Variant can only be derived for a fieldless enum; variant carries data",
            ));
        }
        let value = parse_variant_value(variant)?
            .unwrap_or_else(|| to_snake_case(&variant.ident.to_string()));
        idents.push(&variant.ident);
        values.push(value);
    }

    let core = core_path();

    Ok(quote! {
        impl #core::Variant for #name {
            const KEY: &'static str = #key;
            const OPTIONS: &'static [Self] = &[ #( Self::#idents ),* ];

            fn value(&self) -> &'static str {
                match self {
                    #( Self::#idents => #values, )*
                }
            }

            fn from_value(value: &str) -> ::core::option::Option<Self> {
                match value {
                    #( #values => ::core::option::Option::Some(Self::#idents), )*
                    _ => ::core::option::Option::None,
                }
            }
        }

        impl #core::model::sql::SqlType for #name {
            const SQL: &'static str = "TEXT";
        }
    })
}

fn parse_enum_key(input: &DeriveInput) -> syn::Result<Option<String>> {
    let mut key = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("variant") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("key") {
                let value: LitStr = meta.value()?.parse()?;
                key = Some(value.value());
                Ok(())
            } else {
                Err(meta
                    .error("unknown `#[variant(...)]` attribute on an enum; valid: key = \"…\""))
            }
        })?;
    }
    Ok(key)
}

fn parse_variant_value(variant: &syn::Variant) -> syn::Result<Option<String>> {
    let mut value = None;
    for attr in &variant.attrs {
        if !attr.path().is_ident("variant") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("value") {
                let lit: LitStr = meta.value()?.parse()?;
                value = Some(lit.value());
                Ok(())
            } else {
                Err(meta.error(
                    "unknown `#[variant(...)]` attribute on a variant; valid: value = \"…\"",
                ))
            }
        })?;
    }
    Ok(value)
}
