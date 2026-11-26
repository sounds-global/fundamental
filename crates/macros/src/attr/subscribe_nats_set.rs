//! Message-set code generation for `#[subscribe(set)]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ItemFn;

use crate::attr::name_arg::NameArg;
use crate::attr::subscribe_target::first_param_type;
use crate::attr::subscribe_worker::{ListenerArgs, worker_fn};
use crate::derives::nats_path;

/// The handler plus a `{fn}_listener` routing every subject of its `MessageSet` through one dispatch loop; the handler gets the payload.
pub(crate) fn expand(item: ItemFn, queue: Option<NameArg>) -> syn::Result<TokenStream> {
    let nats = nats_path();
    let set_ty = first_param_type(&item.sig)?;
    let handler = &item.sig.ident;
    let vis = &item.vis;
    let listener = format_ident!("{handler}_listener");

    let queue = match queue {
        Some(group) => {
            let name = group.name_expr(&nats, "Queue");
            quote!(.with_queue_group(#name))
        }
        None => quote!(),
    };
    let doc = format!(
        "Subscribes the `{handler}` handler to every subject in its message set and spawns \
         the merged dispatch loop, returning a `Listener` guard."
    );

    let worker = worker_fn(vis, handler, &ListenerArgs::Client);

    Ok(quote! {
        #item

        #[doc = #doc]
        #vis async fn #listener(
            client: &#nats::NatsClient,
        ) -> #nats::NatsResult<#nats::Listener> {
            let options = #nats::SubscriptionOptions::new()#queue;
            client
                .subscribe_set_dispatch::<#set_ty, _, _>(options, move |__message| #handler(__message.payload))
                .await
        }

        #worker
    })
}
