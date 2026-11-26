//! `mount_device_slices! { Mine, Channel }`: the slices a native host syncs and serves; an unlisted slice cannot be read there.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Path, Token, parse_macro_input};

use crate::derives::native_path;

struct MountDeviceSlices {
    slices: Punctuated<Path, Token![,]>,
}

impl Parse for MountDeviceSlices {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            slices: Punctuated::parse_terminated(input)?,
        })
    }
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as MountDeviceSlices);
    let markers: Vec<&Path> = parsed.slices.iter().collect();

    let native = native_path();
    let withs = markers.iter().map(|marker| {
        quote! {
            let registry = registry.with::<#marker>();
        }
    });

    quote! {
        #[cfg(not(target_arch = "wasm32"))]
        /// Builds the device-slice registry listed in `mount_device_slices!`,
        /// handed to the native host's `SyncHost`.
        #[must_use]
        pub fn device_slices() -> #native::DeviceSliceRegistry {
            let registry = #native::DeviceSliceRegistry::new();
            #(#withs)*
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
        let parsed: MountDeviceSlices = parse_quote!(domain::sync::Mine, domain::sync::Channel);
        assert_eq!(parsed.slices.len(), 2);
    }

    #[test]
    fn parses_an_empty_list() {
        let parsed: MountDeviceSlices = syn::parse2(quote! {}).expect("empty list parses");
        assert_eq!(parsed.slices.len(), 0);
    }
}
