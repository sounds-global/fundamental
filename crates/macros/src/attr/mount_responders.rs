//! `mount_responders! { audit_rpc, settle_rpc }`: an app's `#[respond]` handlers, described for introspection.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Path, Token, parse_macro_input};

use crate::derives::nats_path;

struct MountResponders {
    handlers: Punctuated<Path, Token![,]>,
}

impl Parse for MountResponders {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            handlers: Punctuated::parse_terminated(input)?,
        })
    }
}

/// `path::audit_rpc` to the `path::audit_rpc_descriptor` accessor `#[respond]` emits beside it.
fn descriptor_accessor(handler: &Path) -> Path {
    let mut accessor = handler.clone();
    if let Some(last) = accessor.segments.last_mut() {
        last.ident = format_ident!("{}_descriptor", last.ident);
    }
    accessor
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as MountResponders);
    let nats = nats_path();

    let descriptors = parsed
        .handlers
        .iter()
        .map(descriptor_accessor)
        .map(|accessor| quote!(#accessor()));

    quote! {
        #[cfg(not(target_arch = "wasm32"))]
        /// Describes every responder listed in `mount_responders!` — the RPC surface as
        /// data, for dev tooling.
        pub fn describe_responders() -> ::std::vec::Vec<#nats::MessageDescriptor> {
            ::std::vec![#(#descriptors),*]
        }
    }
    .into()
}
