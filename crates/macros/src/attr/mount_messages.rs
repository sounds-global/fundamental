//! `mount_messages! { UserCreated, OrderPlaced; sets: Events }`: the messages an app handles.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, Path, Token, parse_macro_input};

use crate::derives::nats_path;

struct MountMessages {
    messages: Vec<Path>,
    sets: Vec<Path>,
}

impl Parse for MountMessages {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // A set-only form is told apart by the lone `:` after `sets`; a path would continue with `::`, `,` or `;`.
        if input.peek(Ident) && input.peek2(Token![:]) {
            let sets = parse_sets_group(input)?;
            return Ok(Self {
                messages: Vec::new(),
                sets,
            });
        }

        let messages = parse_path_list(input, |i| i.peek(Token![;]))?;

        let mut sets = Vec::new();
        if input.peek(Token![;]) {
            input.parse::<Token![;]>()?;
            sets = parse_sets_group(input)?;
        }

        Ok(Self { messages, sets })
    }
}

fn parse_sets_group(input: ParseStream) -> syn::Result<Vec<Path>> {
    let keyword: Ident = input.parse()?;
    if keyword != "sets" {
        return Err(syn::Error::new(
            keyword.span(),
            "expected `sets:` in `mount_messages!`",
        ));
    }
    input.parse::<Token![:]>()?;
    parse_path_list(input, |_| false)
}

/// A comma-separated path list, stopping at `stop` or the end of input.
fn parse_path_list(
    input: ParseStream,
    stop: impl Fn(ParseStream) -> bool,
) -> syn::Result<Vec<Path>> {
    let mut paths = Vec::new();
    while !input.is_empty() && !stop(input) {
        paths.push(input.parse::<Path>()?);
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        } else {
            break;
        }
    }
    Ok(paths)
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as MountMessages);
    let nats = nats_path();
    let markers: Vec<&Path> = parsed.messages.iter().collect();
    let sets: Vec<&Path> = parsed.sets.iter().collect();

    // An app-local trait, so the impls on foreign message types pass the orphan rule; `#[subscribe]` asserts it.
    let impls = markers
        .iter()
        .map(|marker| quote!(impl MountedMessage for #marker {}));

    let descriptors = markers.iter().map(|marker| {
        quote! {
            #nats::MessageDescriptor::new(
                <#marker as #nats::Message>::SUBJECT,
                #nats::MessageTransport::Nats,
                "",
                #nats::describe!(#marker),
            )
        }
    });

    quote! {
        /// Marks the messages this app mounts. `#[subscribe]` requires a handler's payload
        /// to implement it, so subscribing to an unmounted message does not compile.
        #[doc(hidden)]
        #[diagnostic::on_unimplemented(
            message = "the message `{Self}` is not mounted in this app",
            note = "list it in `mount_messages! {{ ... }}` so this app can subscribe to it"
        )]
        pub trait MountedMessage: #nats::Message {}

        #(#impls)*

        #[cfg(not(target_arch = "wasm32"))]
        /// Describes every message listed in `mount_messages!` — one descriptor per message
        /// and one per set variant — the surface as data, for dev tooling.
        pub fn describe_messages() -> ::std::vec::Vec<#nats::MessageDescriptor> {
            let mut descriptors = ::std::vec![#(#descriptors),*];
            #( descriptors.extend(#sets::describe_set()); )*
            descriptors
        }
    }
    .into()
}
