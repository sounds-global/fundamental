//! `seed_main!`: a `src/bin/seed.rs` entry point that gates the environment and runs the seeds.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Expr, Path, Token, bracketed, parse_macro_input};

use crate::derives::{core_path, seed_path};

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as SeedMainInput);
    match build(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Debug)]
struct SeedMainInput {
    app: Expr,
    context: Expr,
    seeds: Vec<Path>,
}

impl Parse for SeedMainInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut app: Option<Expr> = None;
        let mut context: Option<Expr> = None;
        let mut seeds: Option<Vec<Path>> = None;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "app" {
                app = Some(input.parse()?);
            } else if key == "context" {
                context = Some(input.parse()?);
            } else if key == "seeds" {
                let content;
                bracketed!(content in input);
                let paths = Punctuated::<Path, Token![,]>::parse_terminated(&content)?;
                seeds = Some(paths.into_iter().collect());
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    "unknown `seed_main!` key; valid keys: app, context, seeds",
                ));
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            } else {
                break;
            }
        }

        // Required: the guard must judge the configuration the seeds connect with, which only the seeder loads.
        let app = app.ok_or_else(|| {
            input.error(
                "`seed_main!` requires `app = <expr>`, the `AppSettings` its seeds connect with",
            )
        })?;
        let context =
            context.ok_or_else(|| input.error("`seed_main!` requires `context = <expr>`"))?;
        let seeds = seeds.ok_or_else(|| input.error("`seed_main!` requires `seeds = [..]`"))?;
        Ok(Self {
            app,
            context,
            seeds,
        })
    }
}

/// `module::demo` to the generated `module::demo_seed` accessor.
fn accessor_path(mut path: Path) -> syn::Result<Path> {
    let Some(last) = path.segments.last_mut() else {
        return Err(syn::Error::new(path.span(), "empty seed path"));
    };
    last.ident = format_ident!("{}_seed", last.ident);
    Ok(path)
}

fn build(input: SeedMainInput) -> syn::Result<TokenStream2> {
    let SeedMainInput {
        app,
        context,
        seeds,
    } = input;
    let seed = seed_path();
    let core = core_path();

    let accessors = seeds
        .into_iter()
        .map(accessor_path)
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        fn main() -> ::std::process::ExitCode {
            let __rt = match ::tokio::runtime::Runtime::new() {
                ::core::result::Result::Ok(rt) => rt,
                ::core::result::Result::Err(err) => {
                    ::std::eprintln!("seed: could not start the async runtime: {err}");
                    return ::std::process::ExitCode::FAILURE;
                }
            };

            let __outcome: ::core::result::Result<(), #seed::SeedError> = __rt.block_on(async {
                let __app = #app;
                #seed::guard_loaded_config(
                    __app.config(),
                    &#core::env::EnvVars::process(),
                )?;
                #[allow(unused_variables)]
                let app = &__app;
                let __ctx = #context;
                let __runner = #seed::SeedRunner::new()
                    #( .register( #accessors () ) )* ;
                __runner.run_all(&__ctx).await
            });

            match __outcome {
                ::core::result::Result::Ok(()) => ::std::process::ExitCode::SUCCESS,
                ::core::result::Result::Err(__err) => {
                    use #core::Error as _;
                    ::std::eprintln!("seed failed [{}]: {}", __err.error_code(), __err);
                    let __code = ::core::convert::TryFrom::try_from(__err.exit_code()).unwrap_or(1u8);
                    ::std::process::ExitCode::from(__code)
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;

    fn parse(tokens: TokenStream2) -> syn::Result<SeedMainInput> {
        syn::parse2(tokens)
    }

    #[test]
    fn parses_context_and_seeds() {
        let input = parse(quote! {
            app = AppSettings::load("main")?,
            context = build_ctx().await,
            seeds = [bootstrap, demo::admin]
        })
        .expect("valid invocation");
        assert_eq!(input.seeds.len(), 2);
    }

    #[test]
    fn missing_context_is_rejected() {
        let err = parse(quote!(app = a, seeds = [a])).expect_err("context is required");
        assert!(err.to_string().contains("context"));
    }

    #[test]
    fn missing_app_is_rejected() {
        let err = parse(quote!(context = c, seeds = [a])).expect_err("app is required");
        assert!(err.to_string().contains("requires `app"), "{err}");
    }

    #[test]
    fn missing_seeds_is_rejected() {
        let err = parse(quote!(app = a, context = c)).expect_err("seeds is required");
        assert!(err.to_string().contains("seeds"));
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = parse(quote!(ctx = c, seeds = [a])).expect_err("unknown key");
        assert!(err.to_string().contains("unknown `seed_main!` key"));
    }

    #[test]
    fn accessor_path_suffixes_the_final_segment() {
        let path: Path = syn::parse_str("module::bootstrap").unwrap();
        let rewritten = accessor_path(path).unwrap().to_token_stream().to_string();
        assert_eq!(rewritten, "module :: bootstrap_seed");
    }

    #[test]
    fn build_emits_a_main_that_runs_the_seeds() {
        let input = parse(quote! {
            app = AppSettings::load("main")?,
            context = AppCtx,
            seeds = [bootstrap, demo]
        })
        .unwrap();
        let tokens = build(input).unwrap().to_string();
        assert!(tokens.contains("fn main"));
        assert!(tokens.contains("guard_loaded_config"), "{tokens}");
        let guard = tokens.find("guard_loaded_config").expect("guarded");
        let context = tokens.find("AppCtx").expect("context built");
        assert!(
            guard < context,
            "the guard runs before the context connects: {tokens}"
        );
        assert!(tokens.contains("bootstrap_seed"));
        assert!(tokens.contains("demo_seed"));
        assert!(tokens.contains("run_all"));
    }
}
