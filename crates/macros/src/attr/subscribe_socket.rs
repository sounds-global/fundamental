//! WebSocket code generation for `#[subscribe(socket)]` / `#[subscribe(socket = "…")]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{ItemFn, LitStr};

use crate::attr::subscribe_target::{mount_assertion, second_param_type};
use crate::derives::{nats_path, ws_path};

/// The handler plus a `{fn}_route` for a `WsRouter`: bare `socket` takes a mounted `Message` on its subject; `socket = "action"` takes any payload, ungated.
pub(crate) fn expand(item: ItemFn, action: Option<LitStr>) -> syn::Result<TokenStream> {
    let ws = ws_path();
    let handler = &item.sig.ident;
    let vis = &item.vis;
    let route = format_ident!("{handler}_route");

    let (action_expr, gate, action_doc) = match action {
        // Raw mode: explicit action, any payload, no mount gate.
        Some(action) => (quote!(#action), TokenStream::new(), action.value()),
        // Message mode: the payload's subject is the action, and it must be mounted.
        None => {
            let nats = nats_path();
            let payload = second_param_type(&item.sig)?;
            (
                quote!(<#payload as #nats::Message>::SUBJECT),
                mount_assertion(&payload),
                "its message subject".to_string(),
            )
        }
    };

    let doc = format!(
        "Builds the route binding the `{handler}` handler to {action_doc}, for a `WsRouter`."
    );

    Ok(quote! {
        #item

        #gate

        #[doc = #doc]
        #vis fn #route() -> #ws::Route {
            #ws::Route::new(#action_expr, #handler)
        }
    })
}
