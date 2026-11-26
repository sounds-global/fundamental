//! `#[slice]`: a server resolver bound to a syncable model, mounted by `mount_slices!`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{FnArg, ItemFn, LitStr, Token, Type, Visibility, parse_macro_input};

use crate::derives::{db_path, sync_path, sync_server_path};

/// Entry point for `#[slice(Model)]` or `#[slice(Model, "name")]`.
pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as SliceArgs);
    let item = parse_macro_input!(item as ItemFn);

    match build(args, item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// The parsed `#[slice(...)]` arguments; `name` defaults to the resolver's.
struct SliceArgs {
    model: Type,
    name: Option<LitStr>,
}

impl Parse for SliceArgs {
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let model: Type = input.parse().map_err(|_| {
            input.error(
                "`#[slice]` expects a model type, as in `#[slice(Animal)]` or \
                 `#[slice(Animal, \"mine\")]`",
            )
        })?;
        let mut name = None;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.peek(LitStr) {
                name = Some(input.parse::<LitStr>()?);
                if input.peek(Token![,]) {
                    input.parse::<Token![,]>()?;
                }
            }
        }
        Ok(Self { model, name })
    }
}

fn build(args: SliceArgs, item: ItemFn) -> syn::Result<TokenStream2> {
    if item.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            item.sig.fn_token,
            "a `#[slice]` resolver must be an `async fn`",
        ));
    }

    let marker = marker_arm(&args, &item);
    let server = server_arm(&item);

    Ok(quote! {
        #marker
        #server
    })
}

/// The slice's marker type and its `Slice` impl; what `mount_slices!` lists.
fn marker_arm(args: &SliceArgs, item: &ItemFn) -> TokenStream2 {
    let sync = sync_path();

    let handler = &item.sig.ident;
    let vis = &item.vis;
    let marker = format_ident!("{}", pascal_case(&handler.to_string()));
    let model = &args.model;
    let params = params_type(item, &sync);
    let name: LitStr = args
        .name
        .clone()
        .unwrap_or_else(|| LitStr::new(&handler.to_string(), handler.span()));

    let doc = format!(
        "Typed handle for the `{}` slice — list it in `mount_slices!` to serve it.",
        name.value()
    );

    quote! {
        #[doc = #doc]
        #[allow(dead_code)]
        #vis struct #marker;

        impl #sync::Slice for #marker {
            type Model = #model;
            type Params = #params;
            const NAME: &'static str = #name;
        }
    }
}

/// The `ServerSlice` impl, gated off the browser target, with the resolver nested inside `resolve`.
fn server_arm(item: &ItemFn) -> TokenStream2 {
    let sync = sync_path();
    let sync_server = sync_server_path();
    let db = db_path();

    let marker = format_ident!("{}", pascal_case(&item.sig.ident.to_string()));
    let handler = &item.sig.ident;
    let params = params_type(item, &sync);

    // A resolver without a params argument takes `NoParams`, which is dropped from the call.
    let call = if item.sig.inputs.len() > 1 {
        quote!(#handler(ctx, params))
    } else {
        quote!(#handler(ctx))
    };

    let mut handler_item = item.clone();
    handler_item.vis = Visibility::Inherited;

    quote! {
        #[cfg(not(target_arch = "wasm32"))]
        impl #sync_server::ServerSlice for #marker {
            fn resolve(
                ctx: #sync_server::SliceContext,
                params: #params,
            ) -> impl ::core::future::Future<
                Output = ::core::result::Result<#db::Filter, #sync_server::SliceError>,
            > + ::core::marker::Send {
                // Imported inside the `wasm32`-gated impl, since these are server-only.
                #[allow(unused_imports)]
                use #sync_server::{SliceContext, SliceError};
                #[allow(unused_imports)]
                use #db::Filter;
                #handler_item
                #call
            }
        }
    }
}

/// The resolver's second parameter type, or `NoParams`.
fn params_type(item: &ItemFn, sync: &TokenStream2) -> TokenStream2 {
    match item.sig.inputs.iter().nth(1) {
        Some(FnArg::Typed(pat)) => {
            let ty = pat.ty.as_ref();
            quote!(#ty)
        }
        _ => quote!(#sync::NoParams),
    }
}

fn pascal_case(snake: &str) -> String {
    snake
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn args(model: Type, name: &str) -> SliceArgs {
        SliceArgs {
            model,
            name: Some(LitStr::new(name, proc_macro2::Span::call_site())),
        }
    }

    fn args_defaulted(model: Type) -> SliceArgs {
        SliceArgs { model, name: None }
    }

    #[test]
    fn emits_a_pascal_marker_and_portable_slice_impl() {
        let item: ItemFn = parse_quote! {
            async fn my_channel(ctx: SliceContext, params: ChannelParams) -> Result<Filter, SliceError> {
                Ok(Filter::eq("channel_id", params.channel_id))
            }
        };
        let expanded = build(args(parse_quote!(Message), "channel"), item)
            .expect("expands")
            .to_string();
        assert!(expanded.contains("struct MyChannel"));
        assert!(expanded.contains("Slice for MyChannel"));
        assert!(expanded.contains("type Model = Message"));
        assert!(expanded.contains("type Params = ChannelParams"));
        assert!(expanded.contains("\"channel\""));
    }

    #[test]
    fn the_server_resolver_impl_is_gated_off_the_browser_and_nests_the_resolver() {
        let item: ItemFn = parse_quote! {
            async fn mine(ctx: SliceContext, params: NoParams) -> Result<Filter, SliceError> {
                Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
            }
        };
        let expanded = build(args(parse_quote!(Animal), "mine"), item)
            .expect("expands")
            .to_string();
        assert!(expanded.contains("cfg (not (target_arch = \"wasm32\"))"));
        assert!(expanded.contains("ServerSlice for Mine"));
        assert!(expanded.contains("fn resolve"));
        assert!(expanded.contains("fn mine"));
        assert!(expanded.contains("mine (ctx , params)"));
    }

    #[test]
    fn omitting_the_params_arg_defaults_to_no_params_and_drops_the_argument() {
        let item: ItemFn = parse_quote! {
            async fn mine(ctx: SliceContext) -> Result<Filter, SliceError> {
                Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
            }
        };
        let expanded = build(args(parse_quote!(Animal), "mine"), item)
            .expect("expands")
            .to_string();
        assert!(expanded.contains("type Params = "));
        assert!(expanded.contains("NoParams"));
        assert!(expanded.contains("mine (ctx)"));
    }

    #[test]
    fn a_non_async_resolver_is_an_error() {
        let item: ItemFn = parse_quote! {
            fn mine(ctx: SliceContext, params: NoParams) -> Result<Filter, SliceError> {
                Ok(Filter::eq("owner_id", "x"))
            }
        };
        let err = build(args(parse_quote!(Animal), "mine"), item).expect_err("must be async");
        assert!(err.to_string().contains("must be an `async fn`"));
    }

    #[test]
    fn pascal_case_joins_word_boundaries() {
        assert_eq!(pascal_case("mine"), "Mine");
        assert_eq!(pascal_case("my_channel"), "MyChannel");
    }

    #[test]
    fn the_name_defaults_to_the_resolver_name_and_a_literal_overrides_it() {
        let item: ItemFn = parse_quote! {
            async fn mine(ctx: SliceContext) -> Result<Filter, SliceError> {
                Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
            }
        };
        let defaulted = build(args_defaulted(parse_quote!(Animal)), item.clone())
            .expect("expands")
            .to_string();
        assert!(defaulted.contains("const NAME"));
        assert!(defaulted.contains("\"mine\""));
        let overridden = build(args(parse_quote!(Animal), "custom"), item)
            .expect("expands")
            .to_string();
        assert!(overridden.contains("\"custom\""));
        assert!(!overridden.contains("\"mine\""));
    }

    #[test]
    fn parses_with_and_without_a_name_override() {
        let defaulted: SliceArgs = parse_quote!(Animal);
        assert!(defaulted.name.is_none());

        let named: SliceArgs = parse_quote!(Animal, "mine");
        assert_eq!(named.name.expect("a name").value(), "mine");

        let trailing: SliceArgs = parse_quote!(Animal,);
        assert!(trailing.name.is_none());
    }
}
