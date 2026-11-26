//! `mount_slices! { Mine, Channel }`: a surface's slices registered from one list.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Path, Token, parse_macro_input};

use crate::derives::{sync_path, sync_server_path};

struct MountSlices {
    slices: Punctuated<Path, Token![,]>,
}

impl Parse for MountSlices {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            slices: Punctuated::parse_terminated(input)?,
        })
    }
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as MountSlices);
    let markers: Vec<&Path> = parsed.slices.iter().collect();

    let sync = sync_path();
    let sync_server = sync_server_path();

    let defines = markers.iter().map(|marker| {
        quote! {
            let registry = registry.define::<
                <#marker as #sync::Slice>::Model,
                <#marker as #sync::Slice>::Params,
                _, _,
            >(
                <#marker as #sync::Slice>::NAME,
                |ctx, params| <#marker as #sync_server::ServerSlice>::resolve(ctx, params),
            );
        }
    });

    quote! {
        #[cfg(not(target_arch = "wasm32"))]
        /// Registers every slice listed in `mount_slices!` on `registry`, returning it
        /// for a [`SliceServerSource`](::fundamental::sync_server::SliceServerSource).
        #[must_use]
        pub fn register_slices(
            registry: #sync_server::SliceRegistry,
        ) -> #sync_server::SliceRegistry {
            #(#defines)*
            registry
        }
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn parses_a_marker_list() {
        let parsed: MountSlices = parse_quote!(domain::sync::Mine, domain::sync::MySummary);
        assert_eq!(parsed.slices.len(), 2);
    }
}
