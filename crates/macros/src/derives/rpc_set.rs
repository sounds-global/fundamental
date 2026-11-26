//! `#[derive(RpcSet)]`: an enum of request/reply RPCs, `Variant(Request, Reply)`, each on derived subjects.

use std::collections::HashMap;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, Type, Variant, parse_macro_input};

use crate::derives::case::to_dotted_case;
use crate::derives::message::{lit_str, validate_subject};
use crate::derives::message_set::variant_doc;
use crate::derives::nats_path;

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct RpcVariant {
    request: Type,
    reply: Type,
    request_subject: String,
    reply_subject: String,
    subject_span: Span,
    doc: String,
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "RpcSet cannot be derived for a generic type; a set is a fixed enum of RPCs",
        ));
    }

    let data = match &input.data {
        Data::Enum(data) => data,
        Data::Struct(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "RpcSet can only be derived for an enum; each variant pairs a request with its reply",
            ));
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "RpcSet can only be derived for an enum, not a union",
            ));
        }
    };

    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "RpcSet requires at least one variant; an empty set would answer nothing",
        ));
    }

    // Default subjects are the dotted enum and variant names; `#[subject = "…"]` replaces the request's.
    let prefix = to_dotted_case(&name.to_string());
    let variants = data
        .variants
        .iter()
        .map(|variant| parse_variant(variant, &prefix))
        .collect::<syn::Result<Vec<_>>>()?;

    // A shared request subject would make dispatch ambiguous.
    let mut seen: HashMap<&str, Span> = HashMap::new();
    for variant in &variants {
        if seen
            .insert(&variant.request_subject, variant.subject_span)
            .is_some()
        {
            return Err(syn::Error::new(
                variant.subject_span,
                format!(
                    "duplicate request subject \"{}\": two RPCs cannot share a subject",
                    variant.request_subject
                ),
            ));
        }
    }

    let nats = nats_path();

    let message_impls = variants.iter().flat_map(|variant| {
        [
            message_impl(&nats, &variant.request, &variant.request_subject),
            message_impl(&nats, &variant.reply, &variant.reply_subject),
        ]
    });

    let request_subjects = variants.iter().map(|variant| &variant.request_subject);
    let reply_subjects = variants.iter().map(|variant| &variant.reply_subject);
    let descriptors = variants.iter().map(|variant| {
        let subject = &variant.request_subject;
        let request = &variant.request;
        let reply = &variant.reply;
        let doc = &variant.doc;
        quote! {
            #nats::MessageDescriptor::request_reply(
                #subject,
                #doc,
                #nats::describe!(#request),
                #nats::describe!(#reply),
                // The queue group belongs to the `#[respond]` handler, whose own descriptor carries it.
                ::core::option::Option::None,
            )
        }
    });

    Ok(quote! {
        #(#message_impls)*

        impl #nats::RpcSet for #name {
            const REQUEST_SUBJECTS: &'static [&'static str] = &[#(#request_subjects),*];
            const REPLY_SUBJECTS: &'static [&'static str] = &[#(#reply_subjects),*];
        }

        #[cfg(not(target_arch = "wasm32"))]
        impl #name {
            /// Describes each RPC in the set — one request/reply [`MessageDescriptor`] per
            /// variant, for the dev playground and other tooling.
            pub fn describe_set() -> ::std::vec::Vec<#nats::MessageDescriptor> {
                ::std::vec![#(#descriptors),*]
            }
        }
    })
}

/// The same `Message` + `Publish` pair `#[derive(Message)]` emits, so wire behaviour matches.
fn message_impl(nats: &TokenStream2, ty: &Type, subject: &str) -> TokenStream2 {
    quote! {
        impl #nats::Message for #ty {
            const SUBJECT: &'static str = #subject;
        }

        impl #nats::Publish for #ty {
            fn subject(&self) -> ::std::borrow::Cow<'static, str> {
                <Self as #nats::Message>::subject(self)
            }

            fn to_bytes(&self) -> #nats::NatsResult<#nats::Bytes> {
                <Self as #nats::Message>::to_bytes(self)
            }
        }
    }
}

/// Parses one two-field tuple variant: its payloads, subjects and doc.
fn parse_variant(variant: &Variant, prefix: &str) -> syn::Result<RpcVariant> {
    let (request, reply) = match &variant.fields {
        Fields::Unnamed(fields) if fields.unnamed.len() == 2 => {
            (fields.unnamed[0].ty.clone(), fields.unnamed[1].ty.clone())
        }
        _ => {
            return Err(syn::Error::new_spanned(
                variant,
                format!(
                    "RpcSet variant `{ident}` must be a two-field tuple variant pairing request \
                     and reply, e.g. `{ident}(Request, Reply)`",
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

    let (request_subject, subject_span) = match subject {
        Some(literal) => (literal.value(), literal.span()),
        None => (
            format!("{prefix}.{}", to_dotted_case(&variant.ident.to_string())),
            variant.ident.span(),
        ),
    };
    validate_subject(&request_subject, subject_span)?;
    let reply_subject = format!("{request_subject}.reply");
    validate_subject(&reply_subject, subject_span)?;

    Ok(RpcVariant {
        request,
        reply,
        request_subject,
        reply_subject,
        subject_span,
        doc: variant_doc(variant),
    })
}
