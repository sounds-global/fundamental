//! Core-NATS code generation for `#[subscribe]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{ItemFn, LitStr};

use crate::attr::name_arg::NameArg;
use crate::attr::subscribe_target::{mount_assertion, received_message_payload};
use crate::attr::subscribe_worker::{ListenerArgs, worker_fn};
use crate::derives::nats_path;

/// The handler plus a `{fn}_listener`; the subject is the payload's unless `subject` overrides it, as a wildcard must.
pub(crate) fn expand(
    item: ItemFn,
    subject: Option<LitStr>,
    queue: Option<NameArg>,
) -> syn::Result<TokenStream> {
    let nats = nats_path();
    let payload = received_message_payload(&item.sig)?;
    let handler = &item.sig.ident;
    let vis = &item.vis;
    let listener = format_ident!("{handler}_listener");

    let subject = match subject {
        Some(literal) => quote!(#literal),
        None => quote!(<#payload as #nats::Message>::SUBJECT),
    };
    let queue = match queue {
        Some(group) => {
            let name = group.name_expr(&nats, "Queue");
            quote!(.with_queue_group(#name))
        }
        None => quote!(),
    };
    let doc = format!(
        "Subscribes the `{handler}` handler and spawns its dispatch loop, returning a `Listener` guard."
    );

    let mount_assertion = mount_assertion(&payload);
    let worker = worker_fn(vis, handler, &ListenerArgs::Client);

    Ok(quote! {
        #item

        #mount_assertion

        #[doc = #doc]
        #vis async fn #listener(
            client: &#nats::NatsClient,
        ) -> #nats::NatsResult<#nats::Listener> {
            let pattern = #nats::SubjectPattern::new(#subject)?;
            let options = #nats::SubscriptionOptions::new()#queue;
            client.subscribe_dispatch::<#payload, _, _>(pattern, options, #handler).await
        }

        #worker
    })
}
