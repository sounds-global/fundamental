//! `mount_workers! { on_hello, billing::book }`: the `#[subscribe]` handlers an app's worker runs.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Path, Token, parse_macro_input};

use crate::derives::nats_path;

struct MountWorkers {
    handlers: Punctuated<Path, Token![,]>,
}

impl Parse for MountWorkers {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            handlers: Punctuated::parse_terminated(input)?,
        })
    }
}

/// `billing::book` to the `billing::book_worker` starter `#[subscribe]` emits beside it.
fn worker_starter(handler: &Path) -> Path {
    let mut starter = handler.clone();
    if let Some(last) = starter.segments.last_mut() {
        last.ident = format_ident!("{}_worker", last.ident);
    }
    starter
}

fn written(handler: &Path) -> String {
    quote!(#handler).to_string().replace(' ', "")
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as MountWorkers);
    let nats = nats_path();
    let starters = parsed.handlers.iter().map(worker_starter);
    let names = parsed.handlers.iter().map(written);

    quote! {
        #[cfg(not(target_arch = "wasm32"))]
        /// Starts every handler listed in `mount_workers!`, in the order listed, each with what
        /// its form takes from `context`: what `App::worker` runs.
        pub fn register_workers<'a>(
            context: &'a #nats::WorkerContext,
        ) -> #nats::WorkersFuture<'a> {
            ::std::boxed::Box::pin(async move {
                let _ = context;
                ::core::result::Result::Ok(::std::vec![#(#starters(context).await?),*])
            })
        }

        #[cfg(not(target_arch = "wasm32"))]
        /// Every handler listed in `mount_workers!`, as written: the worker's handlers as data,
        /// for dev tooling.
        pub fn describe_workers() -> ::std::vec::Vec<&'static str> {
            ::std::vec![#(#names),*]
        }
    }
    .into()
}
