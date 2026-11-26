//! Code generation for `#[derive(Validate)]`; an enum validates only its active variant, with bare field paths since a tagged enum is flat on the wire.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    DeriveInput, Expr, ExprLit, Fields, Lit, Meta, Variant, punctuated::Punctuated, token::Comma,
};

use super::ValidationGenerate;

pub(super) fn validate_impl(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let core = crate::derives::core_path();
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let whole = whole_rules(input)?;
    let body = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            syn::Fields::Named(fields) => struct_body(&fields.named)?,
            syn::Fields::Unnamed(_) => {
                return Err(syn::Error::new_spanned(
                    input,
                    "Validate can only be derived for structs with named fields",
                ));
            }
            syn::Fields::Unit => {
                return Err(syn::Error::new_spanned(
                    input,
                    "Validate cannot be derived for unit structs",
                ));
            }
        },
        syn::Data::Enum(data) => enum_body(&data.variants)?,
        syn::Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "Validate can only be derived for structs and enums, not unions",
            ));
        }
    };

    Ok(quote! {
        impl #impl_generics #core::validate::Validate for #name #ty_generics #where_clause {
            fn validate(&self) -> ::core::result::Result<(), #core::validate::ValidationErrors> {
                let mut __ctx = #core::validate::ValidationContext::new();

                #body
                #(#whole)*

                __ctx.into_result()
            }
        }
    })
}

/// The type's own `custom` rules over `&Self`, whose errors keep the paths they give.
fn whole_rules(input: &DeriveInput) -> syn::Result<Vec<TokenStream2>> {
    let mut rules = Vec::new();
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("validate"))
    {
        let metas = attr.parse_args_with(Punctuated::<Meta, Comma>::parse_terminated)?;
        for meta in metas {
            let path = match &meta {
                Meta::NameValue(rule) if rule.path.is_ident("custom") => match &rule.value {
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(name),
                        ..
                    }) => name.parse::<syn::Path>().map_err(|_| {
                        syn::Error::new_spanned(
                            &rule.value,
                            "`custom` names a function by its path, e.g. \"check\" or \"crate::rules::check\"",
                        )
                    })?,
                    other => return Err(syn::Error::new_spanned(other, "Expected string literal")),
                },
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "a type takes only `custom = \"function\"`, a rule over the whole value; \
                         every other rule belongs on a field",
                    ));
                }
            };
            rules.push(quote! {
                if let Err(__errs) = #path(self) {
                    __ctx.merge_errors(__errs);
                }
            });
        }
    }
    Ok(rules)
}

fn struct_body(fields: &Punctuated<syn::Field, Comma>) -> syn::Result<TokenStream2> {
    let validations: Vec<TokenStream2> = fields
        .iter()
        .filter_map(|field| {
            let ident = field.ident.as_ref()?;
            ValidationGenerate::field_validation(field, &quote! { &self.#ident }).transpose()
        })
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! { #(#validations)* })
}

/// A `match` over the variants, each arm checking only its own fields by reference.
fn enum_body(variants: &Punctuated<Variant, Comma>) -> syn::Result<TokenStream2> {
    let arms: Vec<TokenStream2> = variants
        .iter()
        .map(|variant| {
            let ident = &variant.ident;
            match &variant.fields {
                Fields::Named(fields) => {
                    // Only fields with rules are bound; unread bindings would warn where the author cannot silence them.
                    let checked: Vec<(&syn::Ident, TokenStream2)> = fields
                        .named
                        .iter()
                        .filter_map(|field| {
                            let name = field.ident.as_ref()?;
                            ValidationGenerate::field_validation(field, &quote! { #name })
                                .transpose()
                                .map(|validation| validation.map(|validation| (name, validation)))
                        })
                        .collect::<syn::Result<Vec<_>>>()?;

                    let bindings = checked.iter().map(|(name, _)| name);
                    let validations = checked.iter().map(|(_, validation)| validation);
                    Ok(quote! {
                        Self::#ident { #(#bindings,)* .. } => { #(#validations)* }
                    })
                }
                // A variant with no fields has nothing to check; refusing it would block the shape it exists for.
                Fields::Unit => Ok(quote! { Self::#ident => {} }),
                Fields::Unnamed(_) => Err(syn::Error::new_spanned(
                    variant,
                    "Validate needs named fields to put in an error path; \
                     give this variant's fields names, or wrap a struct that has them \
                     and mark the field `#[validate(nested)]`",
                )),
            }
        })
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        match self {
            #(#arms)*
        }
    })
}
