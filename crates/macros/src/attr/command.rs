//! `#[command]`: an async function as a developer command, addressed `<namespace>:<name>`.

use fundamental_core::CommandAddress;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, LitStr, Signature, Type, parse_macro_input};

use crate::derives::command_path;

pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut args = CommandArgs::default();
    let parser = syn::meta::parser(|meta| args.parse(meta));
    parse_macro_input!(attr with parser);
    let item = parse_macro_input!(item as ItemFn);

    match build(args, &item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Default, Debug)]
struct CommandArgs {
    namespace: Option<LitStr>,
    name: Option<LitStr>,
    description: Option<LitStr>,
}

impl CommandArgs {
    fn parse(&mut self, meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
        if meta.path.is_ident("namespace") {
            self.namespace = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("name") {
            self.name = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("description") {
            self.description = Some(meta.value()?.parse()?);
        } else {
            return Err(meta
                .error("unknown `#[command(...)]` key; valid keys: namespace, name, description"));
        }
        Ok(())
    }
}

/// Checks one half of an address with `CommandAddress`, the grammar `fun run` also uses.
fn validate_segment(lit: &LitStr, role: &str) -> syn::Result<String> {
    let value = lit.value();
    CommandAddress::validate_segment(&value).map_err(|objection| {
        syn::Error::new_spanned(
            lit,
            format!(
                "a `#[command]` {role} {objection}; write it in lower-case kebab-case, e.g. `mint`"
            ),
        )
    })?;
    Ok(value)
}

enum Arity {
    ContextOnly,
    ContextAndArgs,
}

fn build(args: CommandArgs, item: &ItemFn) -> syn::Result<TokenStream2> {
    if item.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            item.sig.fn_token,
            "a `#[command]` function must be `async`",
        ));
    }
    let (ctx_ty, arity) = signature(&item.sig)?;

    let fn_name = &item.sig.ident;
    let vis = &item.vis;
    let command = command_path();

    let Some(namespace_lit) = &args.namespace else {
        return Err(syn::Error::new_spanned(
            &item.sig.ident,
            "a `#[command]` must declare a `namespace`, e.g. `#[command(namespace = \"mint\", name = \"operator-token\")]`",
        ));
    };
    let namespace = validate_segment(namespace_lit, "namespace")?;

    // The fallback is the identifier in kebab-case.
    let name = match &args.name {
        Some(lit) => validate_segment(lit, "name")?,
        None => fn_name.to_string().replace('_', "-"),
    };
    let description = match &args.description {
        Some(lit) => {
            let value = lit.value();
            quote!(::core::option::Option::Some(#value))
        }
        None => quote!(::core::option::Option::None),
    };

    let call = match arity {
        Arity::ContextOnly => quote!(|ctx, _args| ::std::boxed::Box::pin(#fn_name(ctx))),
        Arity::ContextAndArgs => {
            quote!(|ctx, args| ::std::boxed::Box::pin(#fn_name(ctx, args)))
        }
    };

    let accessor = format_ident!("{fn_name}_command");
    let doc = format!("Builds the [`Command`] registration for the `{namespace}:{name}` command.");

    Ok(quote! {
        #item

        #[doc = #doc]
        #vis fn #accessor() -> #command::Command<#ctx_ty> {
            #command::Command::new(
                #namespace,
                #name,
                #description,
                #call,
            )
        }
    })
}

/// The context type `C` and arity; a second argument's type is left to the generated call to check.
fn signature(sig: &Signature) -> syn::Result<(&Type, Arity)> {
    let mut inputs = sig.inputs.iter();
    let Some(first) = inputs.next() else {
        return Err(syn::Error::new_spanned(
            &sig.ident,
            "a `#[command]` function must take a `&Context` argument (and optionally a `CommandArgs`)",
        ));
    };
    let ctx_ty = context_type(first)?;

    let arity = match inputs.next() {
        None => Arity::ContextOnly,
        Some(second) => {
            if let FnArg::Receiver(_) = second {
                return Err(syn::Error::new_spanned(
                    second,
                    "a `#[command]` function cannot take `self`",
                ));
            }
            if inputs.next().is_some() {
                return Err(syn::Error::new_spanned(
                    &sig.inputs,
                    "a `#[command]` function takes at most two arguments: `&Context` and an optional `CommandArgs`",
                ));
            }
            Arity::ContextAndArgs
        }
    };
    Ok((ctx_ty, arity))
}

fn context_type(first: &FnArg) -> syn::Result<&Type> {
    let FnArg::Typed(pat) = first else {
        return Err(syn::Error::new_spanned(
            first,
            "a `#[command]` function cannot take `self`",
        ));
    };
    match pat.ty.as_ref() {
        Type::Reference(reference) if reference.mutability.is_none() => Ok(reference.elem.as_ref()),
        Type::Reference(reference) => Err(syn::Error::new_spanned(
            reference,
            "a `#[command]` context must be a shared reference `&Context`, not `&mut`",
        )),
        other => Err(syn::Error::new_spanned(
            other,
            "a `#[command]` function's first argument must be a shared reference `&Context`",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;
    use syn::parse::Parser;

    fn parse_args(tokens: TokenStream2) -> syn::Result<CommandArgs> {
        let mut args = CommandArgs::default();
        syn::meta::parser(|meta| args.parse(meta)).parse2(tokens)?;
        Ok(args)
    }

    fn item(src: &str) -> ItemFn {
        syn::parse_str(src).expect("valid fn source")
    }

    fn in_namespace(namespace: &str) -> CommandArgs {
        CommandArgs {
            namespace: Some(syn::parse_str(&format!(r#""{namespace}""#)).expect("literal")),
            name: None,
            description: None,
        }
    }

    #[test]
    fn empty_args_parse_but_declare_nothing() {
        let args = parse_args(TokenStream2::new()).expect("an empty attribute parses");
        assert!(args.namespace.is_none());
        assert!(args.name.is_none());
        assert!(args.description.is_none());
    }

    #[test]
    fn parses_namespace_name_and_description() {
        let args = parse_args(quote!(
            namespace = "mint",
            name = "operator-token",
            description = "Mint a bearer"
        ))
        .expect("legal");
        assert_eq!(args.namespace.unwrap().value(), "mint");
        assert_eq!(args.name.unwrap().value(), "operator-token");
        assert_eq!(args.description.unwrap().value(), "Mint a bearer");
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = parse_args(quote!(order = 1)).expect_err("unknown key");
        assert!(err.to_string().contains("unknown `#[command(...)]` key"));
        assert!(err.to_string().contains("namespace"));
    }

    #[test]
    fn a_missing_namespace_is_rejected() {
        let err = build(
            CommandArgs::default(),
            &item("async fn c(ctx: &Ctx) -> Result<(), E> { Ok(()) }"),
        )
        .expect_err("a command must declare a namespace");
        assert!(err.to_string().contains("must declare a `namespace`"));
    }

    #[test]
    fn a_namespace_must_be_kebab_case() {
        // One case per rule in `validate_segment`.
        for bad in [
            "",
            "Mint",
            "mint_keys",
            "mint:keys",
            "1mint",
            "mint-",
            "mint--keys",
            "mint keys",
        ] {
            let mut args = CommandArgs::default();
            args.namespace = Some(syn::parse_str(&format!(r#""{bad}""#)).expect("literal"));
            let err = build(
                args,
                &item("async fn c(ctx: &Ctx) -> Result<(), E> { Ok(()) }"),
            )
            .expect_err(&format!("`{bad}` is not a legal namespace"));
            assert!(
                err.to_string().contains("kebab-case"),
                "`{bad}` was refused, but not for its spelling: {err}"
            );
        }
    }

    #[test]
    fn a_name_must_be_kebab_case() {
        let mut args = in_namespace("mint");
        args.name = Some(syn::parse_str(r#""operator_token""#).expect("literal"));
        let err = build(
            args,
            &item("async fn c(ctx: &Ctx) -> Result<(), E> { Ok(()) }"),
        )
        .expect_err("an underscore is not an address character");
        assert!(err.to_string().contains("cannot contain `_`"), "{err}");
    }

    #[test]
    fn non_async_is_rejected() {
        let err = build(
            in_namespace("mint"),
            &item("fn c(ctx: &Ctx) -> Result<(), E> {}"),
        )
        .expect_err("commands must be async");
        assert!(err.to_string().contains("must be `async`"));
    }

    #[test]
    fn missing_argument_is_rejected() {
        let err = build(
            in_namespace("mint"),
            &item("async fn c() -> Result<(), E> {}"),
        )
        .expect_err("a command needs a context argument");
        assert!(err.to_string().contains("&Context"));
    }

    #[test]
    fn three_arguments_are_rejected() {
        let err = build(
            in_namespace("mint"),
            &item("async fn c(ctx: &Ctx, args: CommandArgs, extra: u8) -> Result<(), E> {}"),
        )
        .expect_err("at most two arguments");
        assert!(err.to_string().contains("at most two arguments"));
    }

    #[test]
    fn mutable_reference_is_rejected() {
        let err = build(
            in_namespace("mint"),
            &item("async fn c(ctx: &mut Ctx) -> Result<(), E> {}"),
        )
        .expect_err("the context must be shared");
        assert!(err.to_string().contains("not `&mut`"));
    }

    #[test]
    fn by_value_context_is_rejected() {
        let err = build(
            in_namespace("mint"),
            &item("async fn c(ctx: Ctx) -> Result<(), E> {}"),
        )
        .expect_err("the context must be a reference");
        assert!(err.to_string().contains("shared reference"));
    }

    #[test]
    fn self_argument_is_rejected() {
        let err = build(
            in_namespace("mint"),
            &item("async fn c(&self) -> Result<(), E> {}"),
        )
        .expect_err("a command is a free function");
        assert!(err.to_string().contains("self"));
    }

    #[test]
    fn context_only_emits_accessor_ignoring_args() {
        let tokens = build(
            in_namespace("greet"),
            &item("pub async fn hello(ctx: &AppCtx) -> Result<(), CommandError> { Ok(()) }"),
        )
        .expect("valid command")
        .to_token_stream()
        .to_string();
        assert!(tokens.contains("async fn hello"));
        assert!(tokens.contains("fn hello_command"));
        assert!(tokens.contains("Command :: new"));
        assert!(tokens.contains("\"greet\""));
        assert!(tokens.contains("\"hello\""));
        assert!(tokens.contains("None"));
        assert!(tokens.contains("| ctx , _args |"));
        assert!(tokens.contains("hello (ctx)"));
    }

    #[test]
    fn a_defaulted_name_is_the_kebab_case_identifier() {
        let tokens = build(
            in_namespace("mint"),
            &item(
                "pub async fn operator_token(ctx: &AppCtx) -> Result<(), CommandError> { Ok(()) }",
            ),
        )
        .expect("valid command")
        .to_token_stream()
        .to_string();
        // An identifier and an address are spelled differently.
        assert!(tokens.contains("\"operator-token\""), "{tokens}");
    }

    #[test]
    fn context_and_args_forwards_the_args() {
        let tokens = build(
            CommandArgs {
                namespace: Some(syn::parse_str(r#""index""#).unwrap()),
                name: None,
                description: Some(syn::parse_str(r#""Rebuild""#).unwrap()),
            },
            &item(
                "pub async fn reindex(ctx: &AppCtx, args: CommandArgs) -> Result<(), CommandError> { Ok(()) }",
            ),
        )
        .expect("valid command")
        .to_token_stream()
        .to_string();
        assert!(tokens.contains("fn reindex_command"));
        assert!(tokens.contains("| ctx , args |"));
        assert!(tokens.contains("reindex (ctx , args)"));
        assert!(tokens.contains("\"Rebuild\""));
    }
}
