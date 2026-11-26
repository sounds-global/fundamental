//! `commands_main!`: a `src/bin/commands.rs` entry point that lists the commands or dispatches one.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Expr, Path, Token, bracketed, parse_macro_input};

use crate::derives::{command_path, core_path};

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as CommandsMainInput);
    match build(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Debug)]
struct CommandsMainInput {
    context: Expr,
    commands: Vec<Path>,
}

impl Parse for CommandsMainInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut context: Option<Expr> = None;
        let mut commands: Option<Vec<Path>> = None;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "context" {
                context = Some(input.parse()?);
            } else if key == "commands" {
                let content;
                bracketed!(content in input);
                let paths = Punctuated::<Path, Token![,]>::parse_terminated(&content)?;
                commands = Some(paths.into_iter().collect());
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    "unknown `commands_main!` key; valid keys: context, commands",
                ));
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            } else {
                break;
            }
        }

        let context =
            context.ok_or_else(|| input.error("`commands_main!` requires `context = <expr>`"))?;
        let commands =
            commands.ok_or_else(|| input.error("`commands_main!` requires `commands = [..]`"))?;
        Ok(Self { context, commands })
    }
}

/// `module::reindex` to the generated `module::reindex_command` accessor.
fn accessor_path(mut path: Path) -> syn::Result<Path> {
    let Some(last) = path.segments.last_mut() else {
        return Err(syn::Error::new(path.span(), "empty command path"));
    };
    last.ident = format_ident!("{}_command", last.ident);
    Ok(path)
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn build(input: CommandsMainInput) -> syn::Result<TokenStream2> {
    let CommandsMainInput { context, commands } = input;
    let command = command_path();
    let core = core_path();

    let accessors = commands
        .into_iter()
        .map(accessor_path)
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        fn main() -> ::std::process::ExitCode {
            let __runner = #command::CommandRunner::new()
                #( .register( #accessors () ) )* ;

            // The first non-flag token is the `<namespace>:<name>` address; the rest passes through.
            let __argv: ::std::vec::Vec<::std::string::String> =
                ::std::env::args().skip(1).collect();
            let mut __name: ::core::option::Option<::std::string::String> =
                ::core::option::Option::None;
            let mut __passthrough: ::std::vec::Vec<::std::string::String> = ::std::vec::Vec::new();
            let mut __list = false;
            for __arg in __argv {
                if __name.is_some() {
                    __passthrough.push(__arg);
                } else if __arg == "--list" || __arg == "-l" {
                    __list = true;
                } else if __arg.starts_with('-') {
                    // An unknown leading flag is ignored, so `-h`-style probes fall through to the listing.
                    __list = true;
                } else {
                    __name = ::core::option::Option::Some(__arg);
                }
            }

            // Listing never builds the context, so it needs no database.
            if __list || __name.is_none() {
                match __runner.listings() {
                    ::core::result::Result::Ok(__rows) => {
                        for __row in __rows {
                            ::std::println!("{}", __row.to_line());
                        }
                        return ::std::process::ExitCode::SUCCESS;
                    }
                    ::core::result::Result::Err(__err) => {
                        use #core::Error as _;
                        ::std::eprintln!("commands failed [{}]: {}", __err.error_code(), __err);
                        return ::std::process::ExitCode::FAILURE;
                    }
                }
            }
            let __name = __name.expect("checked above");

            let __rt = match ::tokio::runtime::Runtime::new() {
                ::core::result::Result::Ok(rt) => rt,
                ::core::result::Result::Err(err) => {
                    ::std::eprintln!("commands: could not start the async runtime: {err}");
                    return ::std::process::ExitCode::FAILURE;
                }
            };

            let __outcome: ::core::result::Result<(), #command::CommandError> = __rt.block_on(async {
                let __ctx = #context;
                __runner
                    .run(&__name, &__ctx, #command::CommandArgs::from(__passthrough))
                    .await
            });

            match __outcome {
                ::core::result::Result::Ok(()) => ::std::process::ExitCode::SUCCESS,
                ::core::result::Result::Err(__err) => {
                    use #core::Error as _;
                    ::std::eprintln!("command failed [{}]: {}", __err.error_code(), __err);
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

    fn parse(tokens: TokenStream2) -> syn::Result<CommandsMainInput> {
        syn::parse2(tokens)
    }

    #[test]
    fn parses_context_and_commands() {
        let input = parse(quote! {
            context = build_ctx().await,
            commands = [hello, module::reindex]
        })
        .expect("valid invocation");
        assert_eq!(input.commands.len(), 2);
    }

    #[test]
    fn missing_context_is_rejected() {
        let err = parse(quote!(commands = [a])).expect_err("context is required");
        assert!(err.to_string().contains("context"));
    }

    #[test]
    fn missing_commands_is_rejected() {
        let err = parse(quote!(context = c)).expect_err("commands is required");
        assert!(err.to_string().contains("commands"));
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = parse(quote!(ctx = c, commands = [a])).expect_err("unknown key");
        assert!(err.to_string().contains("unknown `commands_main!` key"));
    }

    #[test]
    fn accessor_path_suffixes_the_final_segment() {
        let path: Path = syn::parse_str("module::reindex").unwrap();
        let rewritten = accessor_path(path).unwrap().to_token_stream().to_string();
        assert_eq!(rewritten, "module :: reindex_command");
    }

    #[test]
    fn build_emits_a_main_that_lists_and_dispatches() {
        let input = parse(quote! {
            context = AppCtx,
            commands = [hello, reindex]
        })
        .unwrap();
        let tokens = build(input).unwrap().to_string();
        assert!(tokens.contains("fn main"));
        assert!(tokens.contains("CommandRunner :: new"));
        assert!(tokens.contains("hello_command"));
        assert!(tokens.contains("reindex_command"));
        assert!(tokens.contains("listings"));
        assert!(tokens.contains("run"));
    }

    #[test]
    fn list_branch_precedes_context_construction() {
        // `main` lists before it evaluates the context, so `--list` needs no database.
        let input = parse(quote! {
            context = AppCtx::connect().await?,
            commands = [hello]
        })
        .unwrap();
        let rendered = build(input).unwrap().to_string();
        let list_at = rendered.find("listings").expect("lists commands");
        let ctx_at = rendered
            .find("block_on")
            .expect("builds context in block_on");
        assert!(
            list_at < ctx_at,
            "the --list branch must come before context construction"
        );
    }
}
