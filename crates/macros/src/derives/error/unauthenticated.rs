//! The `unauthenticated()` constructor, from the first `status = 401` variant.

use super::VariantInfo;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Fields;

/// `unauthenticated()` for the `status = 401` variant, which `#[action]` calls to refuse a request.
pub(super) fn generate_unauthenticated_impl(
    variants_info: &[VariantInfo],
    data_enum: &syn::DataEnum,
) -> TokenStream2 {
    let candidate = variants_info
        .iter()
        .zip(data_enum.variants.iter())
        .find(|(info, _)| info.attrs.status_lit == Some(401));

    let (info, variant) = match candidate {
        Some(pair) => pair,
        None => return quote! {}, // No 401 variant — use trait default (panics at runtime)
    };

    let ident = &info.ident;

    match &variant.fields {
        Fields::Unit => {
            quote! {
                fn unauthenticated() -> Self {
                    Self::#ident
                }
            }
        }
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
            // Filled with a default message.
            quote! {
                fn unauthenticated() -> Self {
                    Self::#ident("Authentication required".into())
                }
            }
        }
        _ => {
            // Other shapes keep the trait default.
            quote! {}
        }
    }
}
