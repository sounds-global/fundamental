//! `#[middleware]`: an `async fn` as a middleware value an `#[action]` lists.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    FnArg, GenericArgument, ItemFn, Path, PathArguments, Type, parse_macro_input, parse_quote,
};

use crate::attr::handler_state::{context_state, state_is_generic};
use crate::derives::server_path;

pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut on: Option<Path> = None;
    let mut declares = Declarations::default();
    if !attr.is_empty() {
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("on") {
                on = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("needs_principal") {
                declares.needs_principal = true;
                Ok(())
            } else if meta.path.is_ident("grants_authorization") {
                declares.grants_authorization = true;
                Ok(())
            } else if meta.path.is_ident("reads_body") {
                declares.reads_body = true;
                Ok(())
            } else {
                Err(meta.error(
                    "unsupported `#[middleware]` argument; expected `on = <Type>`, `needs_principal`, `grants_authorization` or `reads_body`",
                ))
            }
        });
        parse_macro_input!(attr with parser);
    }

    let item = parse_macro_input!(item as ItemFn);
    match build(item, on, declares) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// The associated consts to emit; a struct so two opposite booleans cannot be swapped.
#[derive(Default, Clone, Copy)]
struct Declarations {
    needs_principal: bool,
    grants_authorization: bool,
    reads_body: bool,
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn build(item: ItemFn, on: Option<Path>, declares: Declarations) -> syn::Result<TokenStream2> {
    if item.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            item.sig.fn_token,
            "a `#[middleware]` handler must be an `async fn`",
        ));
    }

    // With `on` the receiver is required: a middleware that needs no state stays the unit form.
    let expected = if on.is_some() { 4 } else { 3 };
    if item.sig.inputs.len() != expected {
        let shape = if on.is_some() {
            "a `#[middleware(on = …)]` handler takes exactly `(&self, ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>)`"
        } else {
            "a `#[middleware]` handler takes exactly `(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>)`"
        };
        return Err(syn::Error::new_spanned(&item.sig.inputs, shape));
    }

    let mut params = item.sig.inputs.iter();
    let receiver = match on {
        Some(_) => match params.next() {
            Some(FnArg::Receiver(receiver)) if receiver.reference.is_some() => Some(receiver),
            Some(other) => {
                return Err(syn::Error::new_spanned(
                    other,
                    "a `#[middleware(on = …)]` handler takes `&self` as its first parameter",
                ));
            }
            None => unreachable!("arity is checked before parameters are read"),
        },
        None => None,
    };
    if let Some(receiver) = receiver
        && receiver.mutability.is_some()
    {
        return Err(syn::Error::new_spanned(
            receiver,
            "a middleware is shared across concurrent requests, so it takes `&self`, never `&mut self`",
        ));
    }

    let context_arg = params.clone().next().ok_or_else(|| {
        syn::Error::new_spanned(&item.sig, "the handler must take `HttpContext<S>`")
    })?;
    let state = context_state(context_arg)?;
    let server = server_path();

    let vis = &item.vis;
    let name = &item.sig.ident;
    let attrs = &item.attrs;
    let body = &item.block;
    let output = &item.sig.output;

    // The handler's own patterns over the canonical types; `next` gets the trait's elided lifetime.
    let ctx_arg = typed(params.next())?;
    let body_arg = typed(params.next())?;
    let next_arg = typed(params.next())?;

    let ctx_pat = &ctx_arg.pat;
    let ctx_ty = &ctx_arg.ty;
    let body_pat = &body_arg.pat;
    let body_ty = &body_arg.ty;
    let next_pat = &next_arg.pat;
    let next_ty = with_anonymous_lifetime((*next_arg.ty).clone());

    let future_output = match output {
        syn::ReturnType::Type(_, ty) => quote!(#ty),
        syn::ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &item.sig,
                "a `#[middleware]` handler must return `MiddlewareResult`",
            ));
        }
    };

    let (impl_generics, where_clause) = if state_is_generic(&item, state) {
        let mut generics = item.sig.generics.clone();
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#state: ::core::marker::Send + ::core::marker::Sync + 'static));
        split(&generics)
    } else {
        (quote!(), quote!())
    };

    // With `on` only the impl is emitted; otherwise the unit struct carries the handler's name.
    let (target, declaration) = match &on {
        Some(path) => (quote!(#path), quote!()),
        None => (
            quote!(#name),
            quote! {
                #(#attrs)*
                #[allow(non_camel_case_types)]
                #vis struct #name;
            },
        ),
    };

    // Emitted only when marked, so each trait default covers the rest.
    let principal_const = declares.needs_principal.then(|| {
        quote! {
            const NEEDS_PRINCIPAL: bool = true;
        }
    });
    let authorization_const = declares.grants_authorization.then(|| {
        quote! {
            const GRANTS_AUTHORIZATION: bool = true;
        }
    });
    let reads_body_const = declares.reads_body.then(|| {
        quote! {
            const READS_BODY: bool = true;
        }
    });

    Ok(quote! {
        #declaration

        impl #impl_generics #server::Middleware<#state> for #target #where_clause {
            #principal_const
            #authorization_const
            #reads_body_const

            fn handle(
                &self,
                #ctx_pat: #ctx_ty,
                #body_pat: #body_ty,
                #next_pat: #next_ty,
            ) -> impl ::core::future::Future<Output = #future_output> + ::core::marker::Send {
                async move #body
            }
        }
    })
}

fn typed(arg: Option<&FnArg>) -> syn::Result<&syn::PatType> {
    match arg {
        Some(FnArg::Typed(pat)) => Ok(pat),
        Some(other) => Err(syn::Error::new_spanned(
            other,
            "a `#[middleware]` handler cannot take `self`",
        )),
        None => unreachable!("arity is checked before parameters are read"),
    }
}

/// Adds `'_` to a type's last segment when it has no lifetime, so `Next<S>` becomes `Next<'_, S>`.
fn with_anonymous_lifetime(mut ty: Type) -> Type {
    if let Type::Path(type_path) = &mut ty
        && let Some(segment) = type_path.path.segments.last_mut()
        && let PathArguments::AngleBracketed(args) = &mut segment.arguments
        && !args
            .args
            .iter()
            .any(|arg| matches!(arg, GenericArgument::Lifetime(_)))
    {
        args.args.insert(0, parse_quote!('_));
    }
    ty
}

/// The `impl` generics and where-clause for the impl.
fn split(generics: &syn::Generics) -> (TokenStream2, TokenStream2) {
    let (ig, _tg, wc) = generics.split_for_impl();
    (quote!(#ig), quote!(#wc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn expand_ok(item: ItemFn) -> String {
        build(item, None, Declarations::default())
            .expect("expands")
            .to_string()
    }

    fn expand_on(item: ItemFn, target: Path) -> String {
        build(item, Some(target), Declarations::default())
            .expect("expands")
            .to_string()
    }

    fn expand_gated(item: ItemFn) -> String {
        build(
            item,
            None,
            Declarations {
                needs_principal: true,
                ..Default::default()
            },
        )
        .expect("expands")
        .to_string()
    }

    #[test]
    fn generic_state_implements_middleware_for_every_state() {
        let item: ItemFn = parse_quote! {
            async fn auth<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        let expanded = expand_ok(item);
        assert!(expanded.contains("struct auth"));
        assert!(expanded.contains("impl < S >"));
        assert!(expanded.contains("Middleware < S >"));
        assert!(expanded.contains("Next < '_ , S >"));
    }

    #[test]
    fn concrete_state_implements_middleware_for_that_state_only() {
        let item: ItemFn = parse_quote! {
            async fn audit(ctx: HttpContext<AppState>, body: Vec<u8>, next: Next<AppState>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        let expanded = expand_ok(item);
        assert!(expanded.contains("struct audit"));
        assert!(expanded.contains("Middleware < AppState >"));
        assert!(!expanded.contains("impl < S >"));
    }

    #[test]
    fn needs_principal_emits_the_declaration() {
        let item: ItemFn = parse_quote! {
            async fn auth<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        assert!(expand_gated(item).contains("const NEEDS_PRINCIPAL : bool = true"));
    }

    /// Absence leaves the trait's `false` default in place.
    #[test]
    fn without_the_marker_no_declaration_is_emitted() {
        let item: ItemFn = parse_quote! {
            async fn audit<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        assert!(!expand_ok(item).contains("NEEDS_PRINCIPAL"));
    }

    #[test]
    fn reads_body_emits_the_declaration() {
        let item: ItemFn = parse_quote! {
            async fn signed<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        let expanded = build(
            item,
            None,
            Declarations {
                reads_body: true,
                ..Default::default()
            },
        )
        .expect("expands")
        .to_string();
        assert!(expanded.contains("const READS_BODY : bool = true"));
    }

    fn expand_authorizing(item: ItemFn) -> String {
        build(
            item,
            None,
            Declarations {
                grants_authorization: true,
                ..Default::default()
            },
        )
        .expect("expands")
        .to_string()
    }

    #[test]
    fn grants_authorization_emits_the_declaration() {
        let item: ItemFn = parse_quote! {
            async fn guard<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        assert!(expand_authorizing(item).contains("const GRANTS_AUTHORIZATION : bool = true"));
    }

    #[test]
    fn without_the_marker_no_authorization_declaration_is_emitted() {
        let item: ItemFn = parse_quote! {
            async fn audit<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        assert!(!expand_ok(item).contains("GRANTS_AUTHORIZATION"));
    }

    #[test]
    fn the_two_declarations_are_independent() {
        let item: ItemFn = parse_quote! {
            async fn guard<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        let authorizing = expand_authorizing(item.clone());
        assert!(authorizing.contains("const GRANTS_AUTHORIZATION : bool = true"));
        assert!(!authorizing.contains("NEEDS_PRINCIPAL"));

        let gated = expand_gated(item);
        assert!(gated.contains("const NEEDS_PRINCIPAL : bool = true"));
        assert!(!gated.contains("GRANTS_AUTHORIZATION"));
    }

    #[test]
    fn needs_principal_composes_with_a_target_type() {
        let item: ItemFn = parse_quote! {
            async fn gate(&self, ctx: HttpContext<AppState>, body: Vec<u8>, next: Next<AppState>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        let expanded = build(
            item,
            Some(parse_quote!(RateLimiter)),
            Declarations {
                needs_principal: true,
                ..Default::default()
            },
        )
        .expect("expands")
        .to_string();
        assert!(expanded.contains("Middleware < AppState > for RateLimiter"));
        assert!(expanded.contains("const NEEDS_PRINCIPAL : bool = true"));
    }

    #[test]
    fn rejects_a_non_async_handler() {
        let item: ItemFn = parse_quote! {
            fn mw<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult { todo!() }
        };
        let err = build(item, None, Declarations::default()).expect_err("must be async");
        assert!(err.to_string().contains("async fn"));
    }

    #[test]
    fn rejects_the_wrong_arity() {
        let item: ItemFn = parse_quote! {
            async fn mw<S>(ctx: HttpContext<S>, next: Next<S>) -> MiddlewareResult { todo!() }
        };
        let err = build(item, None, Declarations::default()).expect_err("needs three params");
        assert!(err.to_string().contains("exactly"));
    }

    #[test]
    fn on_writes_the_impl_against_the_caller_s_type() {
        let item: ItemFn = parse_quote! {
            async fn record<S>(&self, ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                self.requests.inc();
                next.run(ctx, body).await
            }
        };
        let expanded = expand_on(item, parse_quote!(HttpMetrics));

        assert!(expanded.contains("Middleware < S > for HttpMetrics"));
        assert!(
            !expanded.contains("struct record"),
            "the caller owns the type; declaring a second one would shadow it: {expanded}"
        );
        assert!(
            expanded.contains("self . requests . inc ()"),
            "the body must be able to reach the instance: {expanded}"
        );
    }

    #[test]
    fn on_without_a_receiver_is_rejected() {
        let item: ItemFn = parse_quote! {
            async fn record<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        let err = build(
            item,
            Some(parse_quote!(HttpMetrics)),
            Declarations::default(),
        )
        .expect_err("needs `&self`");
        assert!(err.to_string().contains("&self"), "{err}");
    }

    /// One value serves every concurrent request, so `&mut self` would serialize them.
    #[test]
    fn a_mutable_receiver_is_rejected() {
        let item: ItemFn = parse_quote! {
            async fn record<S>(&mut self, ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        let err = build(
            item,
            Some(parse_quote!(HttpMetrics)),
            Declarations::default(),
        )
        .expect_err("must be `&self`");
        assert!(err.to_string().contains("&mut self"), "{err}");
    }

    #[test]
    fn the_unit_form_is_unchanged() {
        let item: ItemFn = parse_quote! {
            async fn auth<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
                next.run(ctx, body).await
            }
        };
        let expanded = expand_ok(item);
        assert!(expanded.contains("struct auth"));
        assert!(expanded.contains("Middleware < S > for auth"));
    }

    #[test]
    fn rejects_a_missing_return_type() {
        let item: ItemFn = parse_quote! {
            async fn mw<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) {}
        };
        let err = build(item, None, Declarations::default()).expect_err("needs a return type");
        assert!(err.to_string().contains("MiddlewareResult"));
    }
}
