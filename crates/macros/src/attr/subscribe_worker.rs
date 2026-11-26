//! The `{fn}_worker` every `#[subscribe]` NATS handler gets beside its `{fn}_listener`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, Visibility};

use crate::derives::{core_path, nats_path};

pub(crate) enum ListenerArgs {
    /// Core NATS (`{fn}_listener(&NatsClient)`), a message or a message set.
    Client,
    /// A JetStream consumer (`{fn}_listener(&JetStream)`).
    JetStream,
    /// An inbox consumer (`{fn}_listener(&JetStream, &Inbox)`); the inbox crate's path.
    Inbox(TokenStream),
}

/// `{handler}_worker(&WorkerContext)`, starting the listener with what its form takes; may go unused when started by hand.
pub(crate) fn worker_fn(vis: &Visibility, handler: &Ident, args: &ListenerArgs) -> TokenStream {
    let nats = nats_path();
    let core = core_path();
    let listener = format_ident!("{handler}_listener");
    let worker = format_ident!("{handler}_worker");
    let name = handler.to_string();
    let doc = format!(
        "Starts the `{handler}` handler's listener from what a worker is subscribed with; \
         `mount_workers!` calls it."
    );
    let start = match args {
        ListenerArgs::Client => quote! {
            #listener(context.client()).await.map_err(#core::AnyError::new)
        },
        ListenerArgs::JetStream => quote! {
            #listener(&context.jetstream()).await.map_err(#core::AnyError::new)
        },
        ListenerArgs::Inbox(inbox) => quote! {
            let inbox = context.get::<#inbox::Inbox>().ok_or_else(|| {
                #core::AnyError::new(#nats::NatsError::WorkerNeeds {
                    worker: ::std::borrow::ToOwned::to_owned(#name),
                    needs: "an Inbox",
                })
            })?;
            #listener(&context.jetstream(), inbox)
                .await
                .map_err(#core::AnyError::new)
        },
    };
    quote! {
        #[doc = #doc]
        #[allow(dead_code, reason = "called by mount_workers!, or not when started by hand")]
        #vis fn #worker<'a>(context: &'a #nats::WorkerContext) -> #nats::ListenerFuture<'a> {
            ::std::boxed::Box::pin(async move { #start })
        }
    }
}
