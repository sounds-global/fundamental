//! `#[derive(MessageSet)]`: an enum of an app's messages, one payload per variant, on compile-time-checked subjects.

use std::collections::HashMap;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, LitStr, Variant, parse_macro_input};

use crate::derives::case::to_dotted_case;
use crate::derives::message::{lit_str, validate_subject};
use crate::derives::nats_path;

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct SetVariant {
    ident: syn::Ident,
    payload: syn::Type,
    subject: String,
    subject_span: Span,
    doc: String,
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "MessageSet cannot be derived for a generic type; a set is a fixed enum of messages",
        ));
    }

    let data = match &input.data {
        Data::Enum(data) => data,
        Data::Struct(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "MessageSet can only be derived for an enum; each variant carries one message payload",
            ));
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "MessageSet can only be derived for an enum, not a union",
            ));
        }
    };

    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "MessageSet requires at least one variant; an empty set would subscribe to nothing",
        ));
    }

    // Default subjects are the dotted enum and variant names; `#[subject = "…"]` replaces the whole default.
    let prefix = to_dotted_case(&name.to_string());
    let variants = data
        .variants
        .iter()
        .map(|variant| parse_variant(variant, &prefix))
        .collect::<syn::Result<Vec<_>>>()?;

    // A shared subject would make `from_parts` ambiguous.
    let mut seen: HashMap<&str, Span> = HashMap::new();
    for variant in &variants {
        if seen
            .insert(&variant.subject, variant.subject_span)
            .is_some()
        {
            return Err(syn::Error::new(
                variant.subject_span,
                format!(
                    "duplicate subject \"{}\": two variants cannot share a subject, dispatch would be ambiguous",
                    variant.subject
                ),
            ));
        }
    }

    let nats = nats_path();

    let subject_arms = variants.iter().map(|variant| {
        let ident = &variant.ident;
        let subject = &variant.subject;
        quote!(#name::#ident(_) => ::std::borrow::Cow::Borrowed(#subject))
    });
    let to_bytes_arms = variants.iter().map(|variant| {
        let ident = &variant.ident;
        quote!(#name::#ident(__inner) => ::std::result::Result::Ok(
            #nats::Bytes::from(#nats::serde_json::to_vec(__inner)?)
        ))
    });
    let subjects = variants.iter().map(|variant| &variant.subject);
    let from_parts_arms = variants.iter().map(|variant| {
        let ident = &variant.ident;
        let subject = &variant.subject;
        quote!(#subject => ::std::result::Result::Ok(
            #name::#ident(#nats::serde_json::from_slice(bytes)?)
        ))
    });
    let descriptors = variants.iter().map(|variant| {
        let subject = &variant.subject;
        let payload = &variant.payload;
        let doc = &variant.doc;
        quote! {
            #nats::MessageDescriptor::new(
                #subject,
                #nats::MessageTransport::Nats,
                #doc,
                #nats::describe!(#payload),
            )
        }
    });

    Ok(quote! {
        impl #nats::Publish for #name {
            fn subject(&self) -> ::std::borrow::Cow<'static, str> {
                match self { #(#subject_arms),* }
            }

            fn to_bytes(&self) -> #nats::NatsResult<#nats::Bytes> {
                match self { #(#to_bytes_arms),* }
            }
        }

        impl #nats::MessageSet for #name {
            const SUBJECTS: &'static [&'static str] = &[#(#subjects),*];

            fn from_parts(subject: &str, bytes: &[u8]) -> #nats::NatsResult<Self> {
                match subject {
                    #(#from_parts_arms,)*
                    __other => ::std::result::Result::Err(#nats::NatsError::unknown_subject(__other)),
                }
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        impl #name {
            /// Describes each message in the set — one [`MessageDescriptor`] per variant, for
            /// dev tooling.
            pub fn describe_set() -> ::std::vec::Vec<#nats::MessageDescriptor> {
                ::std::vec![#(#descriptors),*]
            }
        }
    })
}

/// Parses one variant: its payload, its subject and its doc.
fn parse_variant(variant: &Variant, prefix: &str) -> syn::Result<SetVariant> {
    let payload = match &variant.fields {
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => fields.unnamed[0].ty.clone(),
        _ => {
            return Err(syn::Error::new_spanned(
                variant,
                format!(
                    "MessageSet variant `{ident}` must be a single-field tuple variant carrying its \
                     payload, e.g. `{ident}(Payload)`",
                    ident = variant.ident
                ),
            ));
        }
    };

    let mut subject: Option<LitStr> = None;
    for attr in &variant.attrs {
        if attr.path().is_ident("subject") {
            subject = Some(lit_str(&attr.meta.require_name_value()?.value)?);
        }
    }

    let (subject, subject_span) = match subject {
        Some(literal) => (literal.value(), literal.span()),
        None => (
            format!("{prefix}.{}", to_dotted_case(&variant.ident.to_string())),
            variant.ident.span(),
        ),
    };
    validate_subject(&subject, subject_span)?;

    Ok(SetVariant {
        ident: variant.ident.clone(),
        payload,
        subject,
        subject_span,
        doc: variant_doc(variant),
    })
}

/// A variant's `///` lines joined; shared with `#[derive(RpcSet)]`.
pub(crate) fn variant_doc(variant: &Variant) -> String {
    let lines: Vec<String> = variant
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            syn::Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit),
                    ..
                }) => Some(lit.value().trim().to_string()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    lines.join("\n").trim().to_string()
}
