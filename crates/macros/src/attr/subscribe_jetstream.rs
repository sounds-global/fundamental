//! JetStream code generation for `#[subscribe]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ItemFn;

use crate::attr::name_arg::NameArg;
use crate::attr::subscribe_target::{mount_assertion, received_message_payload};
use crate::attr::subscribe_worker::{ListenerArgs, worker_fn};
use crate::derives::nats_path;

/// The handler plus a `{fn}_listener` on the durable consumer; with `auto_ack` the runtime acks `Ok` and nacks `Err`.
pub(crate) fn expand(
    item: ItemFn,
    stream: NameArg,
    consumer: NameArg,
    auto_ack: bool,
) -> syn::Result<TokenStream> {
    let nats = nats_path();
    let payload = received_message_payload(&item.sig)?;
    let handler = &item.sig.ident;
    let vis = &item.vis;
    let listener = format_ident!("{handler}_listener");
    let doc = format!(
        "Subscribes the `{handler}` handler to its JetStream consumer and spawns its dispatch loop, returning a `Listener` guard."
    );

    let stream = stream.name_expr(&nats, "Stream");
    let consumer = consumer.name_expr(&nats, "Consumer");

    let mount_assertion = mount_assertion(&payload);
    let worker = worker_fn(vis, handler, &ListenerArgs::JetStream);

    Ok(quote! {
        #item

        #mount_assertion

        #[doc = #doc]
        #vis async fn #listener(
            jetstream: &#nats::JetStream,
        ) -> #nats::NatsResult<#nats::Listener> {
            jetstream
                .subscribe_consumer_dispatch::<#payload, _, _>(#stream, #consumer, #auto_ack, #handler)
                .await
        }

        #worker
    })
}
