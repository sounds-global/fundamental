//! `#[projection]`: a recompute handler bound to a projection model and its sources, mounted by `mount_projections!`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, ItemFn, LitStr, Token, Type, Visibility, parenthesized, parse_macro_input};

use crate::derives::{sync_path, sync_server_path};

pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ProjectionArgs);
    let item = parse_macro_input!(item as ItemFn);

    match build(args, item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// The parsed arguments: the projection model, its source models and its stable name.
struct ProjectionArgs {
    model: Type,
    sources: Vec<Type>,
    name: LitStr,
}

impl Parse for ProjectionArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let model: Type = input.parse().map_err(|_| {
            input.error(
                "`#[projection]` expects a projection model, as in \
                 `#[projection(ChannelSummary, sources(Message), \"channel_summary\")]`",
            )
        })?;
        input.parse::<Token![,]>()?;

        let keyword: Ident = input.parse()?;
        if keyword != "sources" {
            return Err(syn::Error::new_spanned(
                keyword,
                "`#[projection]` expects `sources(..)` after the model, listing the \
                 source models whose changes trigger a recompute",
            ));
        }
        let content;
        parenthesized!(content in input);
        let sources: Punctuated<Type, Token![,]> = Punctuated::parse_terminated(&content)?;
        input.parse::<Token![,]>()?;

        let name: LitStr = input.parse()?;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        Ok(Self {
            model,
            sources: sources.into_iter().collect(),
            name,
        })
    }
}

fn build(args: ProjectionArgs, item: ItemFn) -> syn::Result<TokenStream2> {
    if item.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            item.sig.fn_token,
            "a `#[projection]` recompute handler must be an `async fn`",
        ));
    }
    if args.sources.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.sig.ident,
            "`#[projection]` needs at least one source model, as in `sources(Message)`",
        ));
    }

    let sync = sync_path();
    let sync_server = sync_server_path();

    let handler = &item.sig.ident;
    let vis = &item.vis;
    let marker = format_ident!("{}", pascal_case(&handler.to_string()));
    let model = &args.model;
    let sources = &args.sources;
    let name = &args.name;

    let mut handler_item = item.clone();
    handler_item.vis = Visibility::Inherited;

    let doc = format!(
        "Typed handle for the `{}` projection — list it in `mount_projections!` to maintain it.",
        name.value()
    );

    Ok(quote! {
        #[doc = #doc]
        #[allow(dead_code)]
        #vis struct #marker;

        // Named bare inside the `wasm32`-gated impl, since these are server-only.
        #[cfg(not(target_arch = "wasm32"))]
        impl #sync_server::ServerProjection for #marker {
            fn register(
                registry: #sync_server::ProjectionRegistry,
            ) -> #sync_server::ProjectionRegistry {
                #[allow(unused_imports)]
                use #sync_server::{ProjectionContext, ProjectionError, ProjectionUpdate};
                #[allow(unused_imports)]
                use #sync::SyncChange;
                #handler_item
                registry
                    #( .define::<#model, #sources, _, _>(#name, #handler) )*
            }
        }
    })
}

/// `snake_case` to the `PascalCase` marker name; name the handler apart from your models.
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

    fn args(model: Type, sources: Vec<Type>, name: &str) -> ProjectionArgs {
        ProjectionArgs {
            model,
            sources,
            name: LitStr::new(name, proc_macro2::Span::call_site()),
        }
    }

    #[test]
    fn emits_a_marker_and_a_define_per_source() {
        let item: ItemFn = parse_quote! {
            async fn summarize_channel(
                ctx: ProjectionContext,
                change: SyncChange,
            ) -> Result<Vec<ProjectionUpdate>, ProjectionError> {
                Ok(Vec::new())
            }
        };
        let expanded = build(
            args(
                parse_quote!(ChannelSummary),
                vec![parse_quote!(Message), parse_quote!(Membership)],
                "channel_summary",
            ),
            item,
        )
        .expect("expands")
        .to_string();

        assert!(expanded.contains("struct SummarizeChannel"));
        assert!(expanded.contains("ServerProjection for SummarizeChannel"));
        assert!(expanded.contains("fn register"));
        assert!(expanded.contains(":: < ChannelSummary , Message , _ , _ >"));
        assert!(expanded.contains(":: < ChannelSummary , Membership , _ , _ >"));
        assert!(expanded.contains("\"channel_summary\""));
        assert!(expanded.contains("fn summarize_channel"));
        assert!(expanded.contains("(\"channel_summary\" , summarize_channel)"));
    }

    #[test]
    fn the_impl_is_gated_off_the_browser_target() {
        let item: ItemFn = parse_quote! {
            async fn summarize(
                ctx: ProjectionContext,
                change: SyncChange,
            ) -> Result<Vec<ProjectionUpdate>, ProjectionError> {
                Ok(Vec::new())
            }
        };
        let expanded = build(
            args(parse_quote!(Summary), vec![parse_quote!(Message)], "s"),
            item,
        )
        .expect("expands")
        .to_string();
        assert!(expanded.contains("cfg (not (target_arch = \"wasm32\"))"));
    }

    #[test]
    fn a_non_async_handler_is_an_error() {
        let item: ItemFn = parse_quote! {
            fn summarize(
                ctx: ProjectionContext,
                change: SyncChange,
            ) -> Result<Vec<ProjectionUpdate>, ProjectionError> {
                Ok(Vec::new())
            }
        };
        let err = build(
            args(parse_quote!(Summary), vec![parse_quote!(Message)], "s"),
            item,
        )
        .expect_err("must be async");
        assert!(err.to_string().contains("must be an `async fn`"));
    }

    #[test]
    fn no_sources_is_an_error() {
        let item: ItemFn = parse_quote! {
            async fn summarize(
                ctx: ProjectionContext,
                change: SyncChange,
            ) -> Result<Vec<ProjectionUpdate>, ProjectionError> {
                Ok(Vec::new())
            }
        };
        let err =
            build(args(parse_quote!(Summary), Vec::new(), "s"), item).expect_err("needs a source");
        assert!(err.to_string().contains("at least one source"));
    }

    #[test]
    fn pascal_case_joins_word_boundaries() {
        assert_eq!(pascal_case("summarize"), "Summarize");
        assert_eq!(pascal_case("summarize_channel"), "SummarizeChannel");
    }
}
