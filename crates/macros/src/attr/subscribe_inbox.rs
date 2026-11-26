//! Inbox code generation for `#[subscribe(stream, consumer, inbox = "…", isolation = …)]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{FnArg, GenericArgument, ItemFn, LitStr, PathArguments, ReturnType, Type};

use crate::attr::name_arg::NameArg;
use crate::attr::subscribe_target::mount_assertion;
use crate::attr::subscribe_worker::{ListenerArgs, worker_fn};
use crate::derives::{db_path, inbox_path, nats_path};

const SHAPE: &str = "an inbox handler takes exactly `(tx: &mut EffectTransaction<'_>, event: &Inbound<M>)` and returns `Result<(), E>`: the transaction is its only way to write, so its effect commits with the record that it ran";

/// The handler, a `fn`-pointer shim the inbox calls, and a `{fn}_listener(&JetStream, &Inbox)`; a free `fn` taking only the transaction and event captures nothing.
pub(crate) fn expand(
    item: ItemFn,
    stream: NameArg,
    consumer: NameArg,
    effect: &LitStr,
    isolation: &syn::Ident,
) -> syn::Result<TokenStream> {
    let (payload, error) = inbox_signature(&item)?;
    let nats = nats_path();
    let inbox = inbox_path();
    let db = db_path();
    let handler = &item.sig.ident;
    let handler_name = handler.to_string();
    let vis = &item.vis;
    let listener = format_ident!("{handler}_listener");
    let step = format_ident!("__{handler}_inbox_step");
    let stream = stream.name_expr(&nats, "Stream");
    let consumer = consumer.name_expr(&nats, "Consumer");
    let mount_assertion = mount_assertion(&payload);
    let worker = worker_fn(vis, handler, &ListenerArgs::Inbox(inbox.clone()));
    let doc = format!(
        "Starts applying the `{handler}` effect (`{}`) to every delivery of its JetStream consumer, once per event, returning the `Listener` guard.",
        effect.value()
    );
    // `assert!` reads its message as a format string, so braces are doubled.
    let invalid = format!(
        "`{}` is not an effect name: [a-z0-9][a-z0-9._-]{{{{0,99}}}}",
        effect.value()
    );

    Ok(quote! {
        #item

        #mount_assertion

        const _: () = ::core::assert!(#inbox::EffectName::is_valid(#effect), #invalid);

        const _: fn() = || {
            fn __fundamental_assert_event<__M: #nats::Event>() {}
            __fundamental_assert_event::<#payload>();
        };

        #[doc(hidden)]
        fn #step<'a, 't>(
            tx: &'a mut #inbox::EffectTransaction<'t>,
            event: &'a #inbox::Inbound<#payload>,
        ) -> #inbox::StepFuture<'a, #error> {
            ::std::boxed::Box::pin(#handler(tx, event))
        }

        #[doc = #doc]
        #vis async fn #listener(
            jetstream: &#nats::JetStream,
            inbox: &#inbox::Inbox,
        ) -> ::core::result::Result<#nats::Listener, #inbox::InboxError> {
            let binding = #inbox::Binding::new(
                #effect,
                ::core::concat!(::core::module_path!(), "::", #handler_name),
                #stream,
                #consumer,
                #db::IsolationLevel::#isolation,
            )?;
            inbox.listen::<#payload, #error>(jetstream, binding, #step).await
        }

        #worker
    })
}

/// The `M` of `&Inbound<M>` and the `E` of `Result<(), E>`, checking the inbox shape.
fn inbox_signature(item: &ItemFn) -> syn::Result<(Type, Type)> {
    let inputs: Vec<&FnArg> = item.sig.inputs.iter().collect();
    let [FnArg::Typed(tx), FnArg::Typed(event)] = inputs.as_slice() else {
        let span = inputs
            .get(2)
            .map_or_else(|| item.sig.ident.span(), Spanned::span);
        return Err(syn::Error::new(span, SHAPE));
    };
    if !is_reference_to(&tx.ty, "EffectTransaction", true) {
        return Err(syn::Error::new_spanned(&tx.ty, SHAPE));
    }
    let Some(payload) = referenced_generic(&event.ty, "Inbound") else {
        return Err(syn::Error::new_spanned(&event.ty, SHAPE));
    };
    let ReturnType::Type(_, output) = &item.sig.output else {
        return Err(syn::Error::new_spanned(&item.sig, SHAPE));
    };
    let Some(error) = result_error(output) else {
        return Err(syn::Error::new_spanned(output, SHAPE));
    };
    Ok((payload, error))
}

/// Whether `ty` is `&mut Last<..>` (or `&Last<..>` when `mutable` is false).
fn is_reference_to(ty: &Type, last: &str, mutable: bool) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    reference.mutability.is_some() == mutable
        && matches!(&*reference.elem, Type::Path(path)
            if path.path.segments.last().is_some_and(|segment| segment.ident == last))
}

fn referenced_generic(ty: &Type, last: &str) -> Option<Type> {
    if !is_reference_to(ty, last, false) {
        return None;
    }
    let Type::Reference(reference) = ty else {
        return None;
    };
    let Type::Path(path) = &*reference.elem else {
        return None;
    };
    first_type_argument(path.path.segments.last()?)
}

fn result_error(ty: &Type) -> Option<Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    let mut types = arguments.args.iter().filter_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    let ok = types.next()?;
    let error = types.next()?;
    matches!(ok, Type::Tuple(unit) if unit.elems.is_empty()).then(|| error.clone())
}

fn first_type_argument(segment: &syn::PathSegment) -> Option<Type> {
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature_of(source: &str) -> syn::Result<(Type, Type)> {
        inbox_signature(&syn::parse_str::<ItemFn>(source).expect("a function"))
    }

    #[test]
    fn the_inbox_shape_yields_its_payload_and_error() {
        let (payload, error) = signature_of(
            "async fn h(tx: &mut EffectTransaction<'_>, event: &Inbound<DepositPaid>) -> Result<(), BillingError> {}",
        )
        .expect("the inbox shape");
        assert_eq!(quote!(#payload).to_string(), "DepositPaid");
        assert_eq!(quote!(#error).to_string(), "BillingError");
    }

    #[test]
    fn every_other_shape_is_refused() {
        for source in [
            "async fn h(event: &Inbound<D>) -> Result<(), E> {}",
            "async fn h(tx: &mut EffectTransaction<'_>, event: &Inbound<D>, pool: &SqlPool) -> Result<(), E> {}",
            "async fn h(tx: &mut SqlTransaction, event: &Inbound<D>) -> Result<(), E> {}",
            "async fn h(tx: &EffectTransaction<'_>, event: &Inbound<D>) -> Result<(), E> {}",
            "async fn h(tx: &mut EffectTransaction<'_>, event: Inbound<D>) -> Result<(), E> {}",
            "async fn h(tx: &mut EffectTransaction<'_>, event: &Inbound<D>) {}",
            "async fn h(tx: &mut EffectTransaction<'_>, event: &Inbound<D>) -> Result<u8, E> {}",
        ] {
            let error = signature_of(source).expect_err(source);
            assert!(
                error.to_string().contains("an inbox handler takes exactly"),
                "{source}"
            );
        }
    }
}
