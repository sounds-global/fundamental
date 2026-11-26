//! `#[respond]`: a request-reply NATS handler that returns the reply the framework publishes.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, LitStr, ReturnType, Type, parse_macro_input};

use crate::attr::name_arg::NameArg;
use crate::derives::nats_path;

pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut args = RespondArgs::default();
    let parser = syn::meta::parser(|meta| args.parse(meta));
    parse_macro_input!(attr with parser);
    let item = parse_macro_input!(item as ItemFn);

    match build(args, item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// The parsed `#[respond(...)]` keys: an optional subject override and queue group.
#[derive(Default)]
struct RespondArgs {
    subject: Option<LitStr>,
    queue: Option<NameArg>,
}

impl RespondArgs {
    fn parse(&mut self, meta: syn::meta::ParseNestedMeta) -> syn::Result<()> {
        if meta.path.is_ident("subject") {
            self.subject = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("queue") {
            self.queue = Some(meta.value()?.parse()?);
        } else {
            return Err(meta.error("unknown `#[respond(...)]` key; valid keys: subject, queue"));
        }
        Ok(())
    }
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn build(args: RespondArgs, item: ItemFn) -> syn::Result<TokenStream2> {
    if item.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            item.sig.fn_token,
            "a `#[respond]` handler must be an `async fn`",
        ));
    }

    let state = state_type(&item)?;
    let request = request_type(&item)?;
    let reply = match &item.sig.output {
        ReturnType::Type(_, ty) => (**ty).clone(),
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &item.sig,
                "a `#[respond]` handler must return its reply type",
            ));
        }
    };

    let nats = nats_path();
    let handler = &item.sig.ident;
    let vis = &item.vis;
    let responder = format_ident!("{handler}_responder");
    let descriptor = format_ident!("{handler}_descriptor");

    let subject = match &args.subject {
        Some(literal) => quote!(#literal),
        None => quote!(<#request as #nats::Message>::SUBJECT),
    };
    // Emitted twice from one source, to join the group and to describe it, so the descriptor names who receives the message.
    let (queue, queue_descriptor) = match &args.queue {
        Some(group) => {
            let name = group.name_expr(&nats, "Queue");
            (
                quote!(.with_queue_group(#name)),
                quote!(::core::option::Option::Some(#name)),
            )
        }
        None => (quote!(), quote!(::core::option::Option::None)),
    };

    // Bound before the payload is moved out; distinct fields, so the partial moves do not conflict.
    let (meta_binding, meta_argument) = if wants_meta(&item)? {
        (
            quote! {
                let __meta = #nats::RespondMeta::new(__message.headers, __message.subject);
            },
            quote!(, &__meta),
        )
    } else {
        (quote!(), quote!())
    };

    let doc = format!(
        "Serves the `{handler}` RPC: subscribes it, answers each request on its reply inbox, \
         and returns the dispatch `Listener` guard."
    );
    let descriptor_doc = format!(
        "Describes the `{handler}` RPC (subject, request and reply schemas) as a \
         `MessageDescriptor`, for the dev playground and other tooling. List it in \
         `mount_responders! {{ {handler} }}`."
    );

    Ok(quote! {
        #item

        #[doc = #doc]
        #vis async fn #responder(
            client: ::std::sync::Arc<#nats::NatsClient>,
            state: ::std::sync::Arc<#state>,
        ) -> #nats::NatsResult<#nats::Listener> {
            let pattern = #nats::SubjectPattern::new(#subject)?;
            let options = #nats::SubscriptionOptions::new()#queue;
            ::std::sync::Arc::clone(&client)
                .subscribe_dispatch::<#request, _, _>(pattern, options, move |__message| {
                    let state = ::std::sync::Arc::clone(&state);
                    let client = ::std::sync::Arc::clone(&client);
                    async move {
                        // A request without a reply inbox still runs the handler; there is nowhere to answer.
                        #meta_binding
                        let __reply = #handler(&state, __message.payload #meta_argument).await;
                        match __message.reply {
                            ::core::option::Option::Some(__subject) => {
                                client.publish_to(&__subject, __reply).await
                            }
                            ::core::option::Option::None => ::core::result::Result::Ok(()),
                        }
                    }
                })
                .await
        }

        #[cfg(not(target_arch = "wasm32"))]
        #[doc = #descriptor_doc]
        #vis fn #descriptor() -> #nats::MessageDescriptor {
            #nats::MessageDescriptor::request_reply(
                #subject,
                "",
                #nats::describe!(#request),
                #nats::describe!(#reply),
                #queue_descriptor,
            )
        }
    })
}

/// The state type `S` from the handler's first parameter, which must be `&S`.
fn state_type(item: &ItemFn) -> syn::Result<Type> {
    match item.sig.inputs.first() {
        Some(FnArg::Typed(typed)) => match typed.ty.as_ref() {
            Type::Reference(reference) => Ok((*reference.elem).clone()),
            _ => Err(syn::Error::new_spanned(
                &typed.ty,
                "a `#[respond]` handler's first parameter must be the app state by reference, \
                 as in `state: &MyState`",
            )),
        },
        _ => Err(syn::Error::new_spanned(
            &item.sig,
            "a `#[respond]` handler takes `(state: &State, request: Request)`",
        )),
    }
}

fn request_type(item: &ItemFn) -> syn::Result<Type> {
    match item.sig.inputs.iter().nth(1) {
        Some(FnArg::Typed(typed)) => Ok((*typed.ty).clone()),
        _ => Err(syn::Error::new_spanned(
            &item.sig,
            "a `#[respond]` handler takes `(state: &State, request: Request)`",
        )),
    }
}

/// Whether the handler takes the delivery metadata, by reference, as a third parameter.
fn wants_meta(item: &ItemFn) -> syn::Result<bool> {
    match item.sig.inputs.len() {
        2 => Ok(false),
        3 => match item.sig.inputs.iter().nth(2) {
            Some(FnArg::Typed(typed)) if matches!(&*typed.ty, Type::Reference(_)) => Ok(true),
            Some(FnArg::Typed(typed)) => Err(syn::Error::new_spanned(
                &typed.ty,
                "a `#[respond]` handler's third parameter is the delivery metadata by \
                 reference, as in `meta: &RespondMeta`",
            )),
            _ => Err(syn::Error::new_spanned(
                &item.sig,
                "a `#[respond]` handler's third parameter is `meta: &RespondMeta`",
            )),
        },
        _ => Err(syn::Error::new_spanned(
            &item.sig,
            "a `#[respond]` handler takes `(state: &State, request: Request)`, optionally \
             followed by `meta: &RespondMeta`",
        )),
    }
}
