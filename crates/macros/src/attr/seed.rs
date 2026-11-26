//! `#[seed]` — mark an async function as a database seed, generating the
//! `<fn>_seed()` accessor that builds its `fundamental_seed::Seed` registration.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, LitInt, LitStr, Signature, Type, parse_macro_input};

use crate::derives::seed_path;

pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut args = SeedArgs::default();
    let parser = syn::meta::parser(|meta| args.parse(meta));
    parse_macro_input!(attr with parser);
    let item = parse_macro_input!(item as ItemFn);

    match build(args, &item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Default, Debug)]
struct SeedArgs {
    name: Option<LitStr>,
    order: Option<LitInt>,
    after: Option<LitStr>,
}

impl SeedArgs {
    fn parse(&mut self, meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
        if meta.path.is_ident("name") {
            self.name = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("order") {
            self.order = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("after") {
            self.after = Some(meta.value()?.parse()?);
        } else {
            return Err(meta.error("unknown `#[seed(...)]` key; valid keys: name, order, after"));
        }
        Ok(())
    }
}

fn build(args: SeedArgs, item: &ItemFn) -> syn::Result<TokenStream2> {
    if item.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            item.sig.fn_token,
            "a `#[seed]` function must be `async`",
        ));
    }
    let ctx_ty = context_type(&item.sig)?;

    let fn_name = &item.sig.ident;
    let vis = &item.vis;
    let seed = seed_path();

    let name = match &args.name {
        Some(lit) => lit.value(),
        None => fn_name.to_string(),
    };
    let order: u32 = match &args.order {
        Some(lit) => lit.base10_parse()?,
        None => 100,
    };
    let after = match &args.after {
        Some(lit) => {
            let value = lit.value();
            quote!(::core::option::Option::Some(#value))
        }
        None => quote!(::core::option::Option::None),
    };

    let accessor = format_ident!("{fn_name}_seed");
    let doc = format!("Builds the [`Seed`] registration for the `{fn_name}` seed.");

    Ok(quote! {
        #item

        #[doc = #doc]
        #vis fn #accessor() -> #seed::Seed<#ctx_ty> {
            #seed::Seed::new(
                #name,
                #order,
                #after,
                |ctx| ::std::boxed::Box::pin(#fn_name(ctx)),
            )
        }
    })
}

/// The context type `C` of a seed, which takes exactly one `&C`.
fn context_type(sig: &Signature) -> syn::Result<&Type> {
    let mut inputs = sig.inputs.iter();
    let Some(first) = inputs.next() else {
        return Err(syn::Error::new_spanned(
            &sig.ident,
            "a `#[seed]` function must take a single `&Context` argument",
        ));
    };
    if inputs.next().is_some() {
        return Err(syn::Error::new_spanned(
            &sig.inputs,
            "a `#[seed]` function must take exactly one argument, a shared reference `&Context`",
        ));
    }
    let FnArg::Typed(pat) = first else {
        return Err(syn::Error::new_spanned(
            first,
            "a `#[seed]` function cannot take `self`",
        ));
    };
    match pat.ty.as_ref() {
        Type::Reference(reference) if reference.mutability.is_none() => Ok(reference.elem.as_ref()),
        Type::Reference(reference) => Err(syn::Error::new_spanned(
            reference,
            "a `#[seed]` context must be a shared reference `&Context`, not `&mut`",
        )),
        other => Err(syn::Error::new_spanned(
            other,
            "a `#[seed]` function's argument must be a shared reference `&Context`",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;
    use syn::parse::Parser;

    fn parse_args(tokens: TokenStream2) -> syn::Result<SeedArgs> {
        let mut args = SeedArgs::default();
        syn::meta::parser(|meta| args.parse(meta)).parse2(tokens)?;
        Ok(args)
    }

    fn item(src: &str) -> ItemFn {
        syn::parse_str(src).expect("valid fn source")
    }

    #[test]
    fn empty_args_use_defaults() {
        let args = parse_args(TokenStream2::new()).expect("bare #[seed] is legal");
        assert!(args.name.is_none());
        assert!(args.order.is_none());
        assert!(args.after.is_none());
    }

    #[test]
    fn parses_name_order_and_after() {
        let args = parse_args(quote!(name = "boot", order = 5, after = "first"))
            .expect("all keys are legal");
        assert_eq!(args.name.unwrap().value(), "boot");
        assert_eq!(args.order.unwrap().base10_parse::<u32>().unwrap(), 5);
        assert_eq!(args.after.unwrap().value(), "first");
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = parse_args(quote!(priority = 1)).expect_err("unknown key");
        assert!(err.to_string().contains("unknown `#[seed(...)]` key"));
    }

    #[test]
    fn non_async_is_rejected() {
        let err = build(
            SeedArgs::default(),
            &item("fn s(ctx: &Ctx) -> Result<(), E> {}"),
        )
        .expect_err("seeds must be async");
        assert!(err.to_string().contains("must be `async`"));
    }

    #[test]
    fn missing_argument_is_rejected() {
        let err = build(
            SeedArgs::default(),
            &item("async fn s() -> Result<(), E> {}"),
        )
        .expect_err("a seed needs a context argument");
        assert!(err.to_string().contains("single `&Context`"));
    }

    #[test]
    fn extra_argument_is_rejected() {
        let err = build(
            SeedArgs::default(),
            &item("async fn s(ctx: &Ctx, other: u8) -> Result<(), E> {}"),
        )
        .expect_err("a seed takes exactly one argument");
        assert!(err.to_string().contains("exactly one argument"));
    }

    #[test]
    fn mutable_reference_is_rejected() {
        let err = build(
            SeedArgs::default(),
            &item("async fn s(ctx: &mut Ctx) -> Result<(), E> {}"),
        )
        .expect_err("the context must be shared");
        assert!(err.to_string().contains("not `&mut`"));
    }

    #[test]
    fn by_value_argument_is_rejected() {
        let err = build(
            SeedArgs::default(),
            &item("async fn s(ctx: Ctx) -> Result<(), E> {}"),
        )
        .expect_err("the context must be a reference");
        assert!(err.to_string().contains("shared reference"));
    }

    #[test]
    fn self_argument_is_rejected() {
        let err = build(
            SeedArgs::default(),
            &item("async fn s(&self) -> Result<(), E> {}"),
        )
        .expect_err("a seed is a free function");
        assert!(err.to_string().contains("self"));
    }

    #[test]
    fn happy_path_emits_accessor_and_keeps_fn() {
        let tokens = build(
            SeedArgs::default(),
            &item("pub async fn bootstrap(ctx: &AppCtx) -> Result<(), SeedError> { Ok(()) }"),
        )
        .expect("valid seed")
        .to_token_stream()
        .to_string();
        assert!(tokens.contains("async fn bootstrap"));
        assert!(tokens.contains("fn bootstrap_seed"));
        assert!(tokens.contains("Seed :: new"));
        assert!(tokens.contains("\"bootstrap\""));
        assert!(tokens.contains("100u32"));
    }
}
