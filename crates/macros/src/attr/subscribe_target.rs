//! Extracts the message payload type `T` from a `#[subscribe]` handler signature.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{FnArg, GenericArgument, PathArguments, Signature, Type};

/// A compile-time assertion that `payload` implements the app's `MountedMessage`; the closure never runs.
pub(crate) fn mount_assertion(payload: &Type) -> TokenStream {
    quote! {
        const _: fn() = || {
            fn __fundamental_assert_mounted<__M: crate::MountedMessage>() {}
            __fundamental_assert_mounted::<#payload>();
        };
    }
}

/// The `T` in the handler's `ReceivedMessage<T>` argument, or an error.
pub(crate) fn received_message_payload(sig: &Signature) -> syn::Result<Type> {
    for arg in &sig.inputs {
        if let FnArg::Typed(typed) = arg
            && let Some(payload) = received_message_generic(&typed.ty)
        {
            return Ok(payload);
        }
    }
    Err(syn::Error::new_spanned(
        &sig.ident,
        "a `#[subscribe]` handler must take one `ReceivedMessage<T>` argument",
    ))
}

/// The handler's first parameter type: the `MessageSet` a set handler takes.
pub(crate) fn first_param_type(sig: &Signature) -> syn::Result<Type> {
    match sig.inputs.first() {
        Some(FnArg::Typed(typed)) => Ok((*typed.ty).clone()),
        _ => Err(syn::Error::new_spanned(
            &sig.ident,
            "a `#[subscribe(set)]` handler must take one `MessageSet` argument (e.g. `async fn(Events)`)",
        )),
    }
}

/// The handler's second parameter type: the payload a socket handler takes.
pub(crate) fn second_param_type(sig: &Signature) -> syn::Result<Type> {
    match sig.inputs.iter().nth(1) {
        Some(FnArg::Typed(typed)) => Ok((*typed.ty).clone()),
        _ => Err(syn::Error::new_spanned(
            &sig.ident,
            "a socket `#[subscribe]` handler must take `(WsContext, Payload)`",
        )),
    }
}

fn received_message_generic(ty: &Type) -> Option<Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != "ReceivedMessage" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    })
}
