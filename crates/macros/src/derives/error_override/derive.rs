//! Code generation for the `ErrorOverride` derive.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident};

use super::OverrideAttr;

struct OverrideVariant {
    ident: Ident,
    /// Matches the variant without binding fields.
    pattern: TokenStream2,
    attr: OverrideAttr,
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(super) fn error_override_impl(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let core = crate::derives::core_path();
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let data_enum = match &input.data {
        Data::Enum(data) => data,
        Data::Struct(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "ErrorOverride can only be derived for enums",
            ));
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "ErrorOverride cannot be derived for unions",
            ));
        }
    };

    let total_variants = data_enum.variants.len();
    let mut variants: Vec<OverrideVariant> = Vec::new();

    for variant in &data_enum.variants {
        let Some(attr) = OverrideAttr::parse(&variant.attrs)? else {
            continue;
        };
        let pattern = match &variant.fields {
            Fields::Unit => quote! {},
            Fields::Named(_) => quote! { { .. } },
            Fields::Unnamed(_) => quote! { ( .. ) },
        };
        variants.push(OverrideVariant {
            ident: variant.ident.clone(),
            pattern,
            attr,
        });
    }

    // A key no variant declares is omitted, so the trait default `None` applies.
    let mut methods: Vec<TokenStream2> = Vec::new();

    let mut push_method =
        |method: TokenStream2, ret: TokenStream2, arms: Vec<TokenStream2>, declared: usize| {
            if arms.is_empty() {
                return;
            }
            // The catch-all is omitted when every variant declares the key: it would be unreachable.
            let catch_all = if declared == total_variants {
                quote! {}
            } else {
                quote! { _ => None }
            };
            methods.push(quote! {
                fn #method(&self) -> Option<#ret> {
                    match self {
                        #(#arms,)*
                        #catch_all
                    }
                }
            });
        };

    macro_rules! collect {
        ($key:ident, $method:ident, $ret:ty) => {
            collect!($key, $method, $ret, |value| quote! { Some(#value) })
        };
        // `$wrap` adapts the declared value to the method's return type.
        ($key:ident, $method:ident, $ret:ty, $wrap:expr) => {{
            let wrap = $wrap;
            let arms: Vec<TokenStream2> = variants
                .iter()
                .filter_map(|v| {
                    v.attr.$key.as_ref().map(|value| {
                        let ident = &v.ident;
                        let pattern = &v.pattern;
                        let body = wrap(value);
                        quote! { Self::#ident #pattern => #body }
                    })
                })
                .collect();
            let declared = arms.len();
            push_method(quote! { $method }, quote! { $ret }, arms, declared);
        }};
    }

    collect!(code, override_error_code, &'static str);
    collect!(
        message,
        override_display_message,
        ::std::borrow::Cow<'_, str>,
        |value| {
            quote! { Some(::std::borrow::Cow::Borrowed(#value)) }
        }
    );
    collect!(
        developer_message,
        override_developer_message,
        ::std::borrow::Cow<'_, str>,
        |value| {
            quote! { Some(::std::borrow::Cow::Borrowed(#value)) }
        }
    );
    collect!(status, override_http_status_code, u16);
    collect!(retryable, override_is_retryable, bool);
    collect!(exit_code, override_exit_code, i32);

    Ok(quote! {
        impl #impl_generics #core::error::ErrorOverride for #name #ty_generics #where_clause {
            #(#methods)*
        }
    })
}
