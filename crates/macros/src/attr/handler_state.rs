//! The state type `S` of a handler's `HttpContext<S>`, for `#[action]` and `#[middleware]`.

use syn::spanned::Spanned;
use syn::{FnArg, GenericArgument, ItemFn, PathArguments, Type};

pub(crate) fn http_context_state(item: &ItemFn) -> syn::Result<&Type> {
    let first = item.sig.inputs.first().ok_or_else(|| {
        syn::Error::new(
            item.sig.span(),
            "the handler must take `HttpContext<S>` as its first parameter",
        )
    })?;
    context_state(first)
}

/// The `S` of an argument that must be `HttpContext<S>`; a method middleware's context is its second parameter.
pub(crate) fn context_state(arg: &FnArg) -> syn::Result<&Type> {
    let first = arg;

    let FnArg::Typed(pat) = first else {
        return Err(syn::Error::new_spanned(
            first,
            "the handler cannot take `self`",
        ));
    };

    let Type::Path(type_path) = pat.ty.as_ref() else {
        return Err(syn::Error::new_spanned(
            &pat.ty,
            "the first parameter must be `HttpContext<S>`",
        ));
    };

    let segment = type_path.path.segments.last().ok_or_else(|| {
        syn::Error::new_spanned(&pat.ty, "the first parameter must be `HttpContext<S>`")
    })?;
    if segment.ident != "HttpContext" {
        return Err(syn::Error::new_spanned(
            &pat.ty,
            "the first parameter must be `HttpContext<S>`",
        ));
    }

    let PathArguments::AngleBracketed(generics) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "`HttpContext` needs a state type argument, as in `HttpContext<AppState>`",
        ));
    };
    let Some(GenericArgument::Type(state)) = generics.args.first() else {
        return Err(syn::Error::new_spanned(
            generics,
            "`HttpContext` needs a state type argument, as in `HttpContext<AppState>`",
        ));
    };

    Ok(state)
}

/// Whether `state` is one of `item`'s own type parameters: the handler is state-agnostic.
pub(crate) fn state_is_generic(item: &ItemFn, state: &Type) -> bool {
    let Type::Path(type_path) = state else {
        return false;
    };
    let Some(ident) = type_path.path.get_ident() else {
        return false;
    };
    item.sig
        .generics
        .type_params()
        .any(|param| &param.ident == ident)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;
    use syn::parse_quote;

    #[test]
    fn extracts_the_state_type_from_http_context() {
        let item: ItemFn = parse_quote! {
            async fn handler(ctx: HttpContext<AppState>, input: In) -> Result<Out, AnyError> {
                Ok(Out)
            }
        };
        let state = http_context_state(&item).expect("state extracted");
        assert_eq!(quote!(#state).to_string(), "AppState");
    }

    #[test]
    fn rejects_a_first_param_that_is_not_http_context() {
        let item: ItemFn = parse_quote! {
            async fn handler(input: In) -> Result<Out, AnyError> { Ok(Out) }
        };
        let err = http_context_state(&item).expect_err("not an HttpContext");
        assert!(err.to_string().contains("HttpContext<S>"));
    }

    #[test]
    fn rejects_http_context_without_a_state_type() {
        let item: ItemFn = parse_quote! {
            async fn handler(ctx: HttpContext, input: In) -> Result<Out, AnyError> { Ok(Out) }
        };
        let err = http_context_state(&item).expect_err("missing state type");
        assert!(err.to_string().contains("state type argument"));
    }

    #[test]
    fn detects_a_generic_state_parameter() {
        let generic: ItemFn = parse_quote! {
            async fn mw<S>(ctx: HttpContext<S>) -> R { todo!() }
        };
        let state = http_context_state(&generic).expect("state");
        assert!(state_is_generic(&generic, state));

        let concrete: ItemFn = parse_quote! {
            async fn mw(ctx: HttpContext<AppState>) -> R { todo!() }
        };
        let state = http_context_state(&concrete).expect("state");
        assert!(!state_is_generic(&concrete, state));
    }
}
