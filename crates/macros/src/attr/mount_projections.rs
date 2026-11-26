//! `mount_projections! { SummarizeChannel }`: a surface's projections registered from one list; none has a device side.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Path, Token, parse_macro_input};

use crate::derives::sync_server_path;

struct MountProjections {
    projections: Punctuated<Path, Token![,]>,
}

impl Parse for MountProjections {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            projections: Punctuated::parse_terminated(input)?,
        })
    }
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as MountProjections);
    let markers: Vec<&Path> = parsed.projections.iter().collect();

    let sync_server = sync_server_path();

    let registrations = markers.iter().map(|marker| {
        quote! {
            let registry =
                <#marker as #sync_server::ServerProjection>::register(registry);
        }
    });

    let expanded: TokenStream2 = quote! {
        #[cfg(not(target_arch = "wasm32"))]
        /// Registers every projection listed in `mount_projections!` on `registry`,
        /// returning it for the reactive refresh loop.
        #[must_use]
        pub fn register_projections(
            registry: #sync_server::ProjectionRegistry,
        ) -> #sync_server::ProjectionRegistry {
            #(#registrations)*
            registry
        }
    };
    expanded.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn chains_register_per_marker() {
        let input: MountProjections = parse_quote!(SummarizeChannel, UnreadCounts);
        let markers: Vec<&Path> = input.projections.iter().collect();
        assert_eq!(markers.len(), 2);
    }
}
