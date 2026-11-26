//! `migrate_main!`: an app's `src/bin/migrate.rs` entry point over `MigrateRunner` and `run_main`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, LitStr, Path, Token, bracketed, parenthesized, parse_macro_input};

use crate::derives::{core_path, db_path, migrator_path, observability_path};

/// Entry point for `migrate_main! { migrations_dir, guard, sql = (<conn>, [Model, ...]), embedded, … }`.
pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as MigrateMainInput);
    build(input).into()
}

#[derive(Debug)]
struct Backend {
    conn: Expr,
    models: Vec<Path>,
}

#[derive(Debug)]
struct MigrateMainInput {
    app: LitStr,
    telemetry: Option<Expr>,
    migrations_dir: Expr,
    guard: Expr,
    sql: Option<Backend>,
    cql: Option<Backend>,
    offline: Option<Backend>,
    /// The dependencies' `EmbeddedMigrations` sets, in apply order.
    embedded: Vec<Expr>,
    /// Or one iterable of them all, so the binary and the app's tests share a list.
    embedded_list: Option<Expr>,
}

impl Parse for MigrateMainInput {
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut app: Option<LitStr> = None;
        let mut telemetry: Option<Expr> = None;
        let mut migrations_dir: Option<Expr> = None;
        let mut guard: Option<Expr> = None;
        let mut sql = None;
        let mut cql = None;
        let mut offline = None;
        let mut embedded: Option<Vec<Expr>> = None;
        let mut embedded_list: Option<Expr> = None;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "app" {
                app = Some(input.parse()?);
            } else if key == "telemetry" {
                telemetry = Some(input.parse()?);
            } else if key == "migrations_dir" {
                migrations_dir = Some(input.parse()?);
            } else if key == "guard" {
                guard = Some(input.parse()?);
            } else if key == "sql" {
                sql = Some(parse_backend(input)?);
            } else if key == "cql" {
                cql = Some(parse_backend(input)?);
            } else if key == "offline" {
                offline = Some(parse_backend(input)?);
            } else if key == "embedded" && !input.peek(syn::token::Bracket) {
                embedded_list = Some(input.parse()?);
            } else if key == "embedded" {
                let sets;
                bracketed!(sets in input);
                embedded = Some(
                    Punctuated::<Expr, Token![,]>::parse_terminated(&sets)?
                        .into_iter()
                        .collect(),
                );
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    "unknown `migrate_main!` key; valid keys: app, telemetry, migrations_dir, \
                     guard, sql, cql, offline, embedded",
                ));
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            } else {
                break;
            }
        }

        // Required: a migrator's failures go nowhere without its config and telemetry.
        let app = app.ok_or_else(|| {
            input.error(
                "`migrate_main!` requires `app = \"<name>\"`: the app whose configuration and \
                 telemetry the migrator boots",
            )
        })?;
        let migrations_dir = migrations_dir
            .ok_or_else(|| input.error("`migrate_main!` requires `migrations_dir = <expr>`"))?;
        // Required with no default: a default would apply one app's policy to every other.
        let guard = guard.ok_or_else(|| {
            input.error(
                "`migrate_main!` requires `guard = <expr>`: a `MigrationGuard` this app's \
                 migrations are held to",
            )
        })?;
        if sql.is_none() && cql.is_none() && offline.is_none() {
            return Err(
                input.error("`migrate_main!` needs at least one backend (sql, cql, or offline)")
            );
        }
        let embedded = embedded.unwrap_or_default();
        // Embedded sets are SQL-only; without a `sql` backend they would join the drift check and never be created.
        if (!embedded.is_empty() || embedded_list.is_some()) && sql.is_none() {
            return Err(input.error(
                "`migrate_main!` `embedded = [...]` needs `sql = (<conn>, [...])`: an embedded \
                 set's tables are created on the SQL wire",
            ));
        }
        Ok(Self {
            app,
            telemetry,
            migrations_dir,
            guard,
            sql,
            cql,
            offline,
            embedded,
            embedded_list,
        })
    }
}

fn parse_backend(input: ParseStream<'_>) -> syn::Result<Backend> {
    let content;
    parenthesized!(content in input);
    let conn: Expr = content.parse()?;
    content.parse::<Token![,]>()?;
    let models_content;
    bracketed!(models_content in content);
    let models = Punctuated::<Path, Token![,]>::parse_terminated(&models_content)?
        .into_iter()
        .collect();
    Ok(Backend { conn, models })
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn build(input: MigrateMainInput) -> TokenStream2 {
    let MigrateMainInput {
        app,
        telemetry,
        migrations_dir,
        guard,
        sql,
        cql,
        offline,
        embedded,
        embedded_list,
    } = input;
    let migrator = migrator_path();
    let db = db_path();
    let core = core_path();
    let observability = observability_path();
    let customise = match telemetry {
        Some(customise) => quote! { (#customise)(__builder) },
        None => quote! { __builder },
    };

    let split = |b: Option<Backend>| match b {
        Some(b) => (Some(b.conn), b.models),
        None => (None, Vec::new()),
    };
    let (cr, cr_models) = split(sql);
    let (sc, sc_models) = split(cql);
    let (off, off_models) = split(offline);

    let register_cr = cr_models.iter().map(|m| quote! { .sql_model::<#m>() });
    let register_sc = sc_models.iter().map(|m| quote! { .cql_model::<#m>() });
    let register_off = off_models.iter().map(|m| quote! { .offline_model::<#m>() });
    let register_embedded = embedded.iter().map(|set| quote! { .with_embedded(#set) });
    let register_list = embedded_list.map(|list| {
        quote! {
            let __runner = ::core::iter::IntoIterator::into_iter(#list)
                .fold(__runner, |__runner, __set| __runner.with_embedded(__set));
        }
    });

    // Each runner future is erased to `dyn Future`: the concrete futures overflow the compiler's query depth, and `Box::pin` alone does not help.
    let erased = |call: TokenStream2| {
        quote! {
            {
                let __future: ::core::pin::Pin<
                    ::std::boxed::Box<
                        dyn ::core::future::Future<Output = #db::migration::MigrationResult<_>> + '_,
                    >,
                > = ::std::boxed::Box::pin(#call);
                __future
            }
            .await
        }
    };
    let check_block = |conn: &Option<Expr>, engine: &str, method: TokenStream2| {
        let call = erased(quote!(__runner.#method(&__conn)));
        conn.as_ref().map(|c| {
            quote! {
                {
                    let __conn = #c;
                    let __check = #call?;
                    ::std::println!("{}: {}", #engine, __check.summary());
                }
            }
        })
    };
    let apply_block = |conn: &Option<Expr>, engine: &str, method: TokenStream2| {
        let call = erased(quote!(__runner.#method(&__conn)));
        conn.as_ref().map(|c| {
            quote! {
                {
                    let __conn = #c;
                    let __applied = #call?;
                    ::std::println!("{}: applied {} migration(s)", #engine, __applied.len());
                }
            }
        })
    };
    // An on-device store has no step ledger, so it keeps its count.
    let ledger_block = |conn: &Option<Expr>, engine: &str, method: TokenStream2| {
        let call = erased(quote!(__runner.#method(&__conn)));
        conn.as_ref().map(|c| {
            quote! {
                {
                    let __conn = #c;
                    let __ledger = #call?;
                    ::std::print!("{}\n{}", #engine, __ledger);
                }
            }
        })
    };
    let status_block = |conn: &Option<Expr>, engine: &str, method: TokenStream2| {
        let call = erased(quote!(__runner.#method(&__conn)));
        conn.as_ref().map(|c| {
            quote! {
                {
                    let __conn = #c;
                    let __applied = #call?;
                    ::std::println!("{}: {} migration(s) applied", #engine, __applied.len());
                }
            }
        })
    };
    let resolve_block = |conn: &Option<Expr>, engine: &str, method: TokenStream2| {
        let call = erased(quote!(__runner.#method(&__conn, &__name, __act, &__resolution)));
        conn.as_ref().map(|c| {
            quote! {
                if !__found {
                    let __conn = #c;
                    match #call {
                        ::core::result::Result::Ok(__settled) => {
                            __found = true;
                            match __act {
                                #db::migration::Resolve::Retry => ::std::println!(
                                    "{}: {} resolved (retry) by {}: steps {}-{} applied",
                                    #engine, __name, __resolution.by(), __settled.step, __settled.total,
                                ),
                                #db::migration::Resolve::Applied => ::std::println!(
                                    "{}: {} resolved (applied) by {}: the live catalog shows it",
                                    #engine, __name, __resolution.by(),
                                ),
                            }
                        }
                        ::core::result::Result::Err(#db::migration::MigrationError::NotFound(_)) => {}
                        ::core::result::Result::Err(__error) => {
                            return ::core::result::Result::Err(__error);
                        }
                    }
                }
            }
        })
    };

    let cr_check = check_block(&cr, "sql", quote!(check_sql));
    let sc_check = check_block(&sc, "cql", quote!(check_cql));
    let off_check = check_block(&off, "offline", quote!(check_offline));
    let cr_apply = apply_block(&cr, "sql", quote!(apply_sql));
    let sc_apply = apply_block(&sc, "cql", quote!(apply_cql));
    let off_apply = apply_block(&off, "offline", quote!(apply_offline));
    let cr_status = ledger_block(&cr, "sql", quote!(status_sql));
    // Only SQL migrations have committed snapshots to verify against.
    let verify_call = erased(quote!(__runner.verify_sql(&__conn, &__base)));
    let cr_verify = match &cr {
        Some(c) => quote! {
            {
                let __conn = #c;
                let __verified = #verify_call?;
                ::std::println!("sql: {}", __verified.summary());
            }
        },
        None => quote! {
            ::core::result::Result::<(), #db::migration::MigrationError>::Err(
                #db::migration::MigrationError::Unverified {
                    message: ::std::string::ToString::to_string(
                        "verify replays the SQL migrations, and this app has no `sql` backend",
                    ),
                    differences: ::std::vec::Vec::new(),
                },
            )?;
        },
    };
    let verify_arm = if cr.is_some() {
        quote! { #migrator::MigrateCommand::Verify { base: __base } => { #cr_verify } }
    } else {
        quote! { #migrator::MigrateCommand::Verify { .. } => { #cr_verify } }
    };
    let sc_status = ledger_block(&sc, "cql", quote!(status_cql));
    let cr_resolve = resolve_block(&cr, "sql", quote!(resolve_sql));
    let sc_resolve = resolve_block(&sc, "cql", quote!(resolve_cql));
    let resolve_arm = if cr.is_some() || sc.is_some() {
        quote! {
            #migrator::MigrateCommand::Resolve { name: __name, act: __act, reason: __reason } => {
                // Read from the environment, never the command line.
                let __resolution = #db::migration::Resolution::new(&__reason)?;
                let mut __found = false;
                #cr_resolve
                #sc_resolve
                if !__found {
                    return ::core::result::Result::Err(
                        #db::migration::MigrationError::NotFound(__name),
                    );
                }
            }
        }
    } else {
        quote! {
            #migrator::MigrateCommand::Resolve { name: __name, .. } => {
                return ::core::result::Result::Err(
                    #db::migration::MigrationError::NotFound(__name),
                );
            }
        }
    };
    let off_status = status_block(&off, "offline", quote!(applied_offline));

    quote! {
        fn main() -> ::std::process::ExitCode {
            // Inside the runtime: the OTLP exporter's `tonic` client panics without a reactor.
            let __rt = match ::tokio::runtime::Runtime::new() {
                ::core::result::Result::Ok(rt) => rt,
                ::core::result::Result::Err(err) => {
                    ::std::eprintln!("migrate: could not start the async runtime: {err}");
                    return ::std::process::ExitCode::FAILURE;
                }
            };
            let __entered = __rt.enter();
            // In `boot`'s order, so any later failure is logged with its code.
            let (__app, __telemetry) = match #observability::ObservabilityBuilder::boot(#app) {
                ::core::result::Result::Ok((__app, __builder)) => match (#customise).init() {
                    ::core::result::Result::Ok(__guard) => (__app, __guard),
                    ::core::result::Result::Err(__err) => {
                        use #core::Error as _;
                        ::std::eprintln!(
                            "migrate: telemetry did not start [{}]: {}",
                            __err.error_code(),
                            __err
                        );
                        return ::std::process::ExitCode::FAILURE;
                    }
                },
                ::core::result::Result::Err(__err) => {
                    use #core::Error as _;
                    ::std::eprintln!(
                        "migrate: the configuration did not load [{}]: {}",
                        __err.error_code(),
                        __err
                    );
                    return ::std::process::ExitCode::FAILURE;
                }
            };
            // Named for the connection expressions, which may read it.
            let app = &__app;
            let __args: ::std::vec::Vec<::std::string::String> = ::std::env::args().collect();
            let __exit = #migrator::run_main(__args.get(1..).unwrap_or_default(), |__command| {
                __rt.block_on(async {
                    let __env = app.environment();
                    let __config = #db::migration::MigrationConfig {
                        migrations_dir: ::std::convert::Into::into(#migrations_dir),
                        environment: ::std::string::ToString::to_string(app.config().environment_name()),
                        is_protected: __env.is_protected(),
                    };
                    let __runner = #migrator::MigrateRunner::new(__config, #guard)
                        #(#register_embedded)*;
                    #register_list
                    let __runner = __runner
                        #(#register_cr)*
                        #(#register_sc)*
                        #(#register_off)*;

                    match __command {
                        #migrator::MigrateCommand::Check => { #cr_check #sc_check #off_check }
                        #migrator::MigrateCommand::Apply => { #cr_apply #sc_apply #off_apply }
                        #migrator::MigrateCommand::Status => { #cr_status #sc_status #off_status }
                        #verify_arm
                        #resolve_arm
                    }
                    ::core::result::Result::Ok::<(), #db::migration::MigrationError>(())
                })
            });
            ::core::mem::drop(__telemetry);
            ::core::mem::drop(__entered);
            __exit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn parse(tokens: TokenStream2) -> syn::Result<MigrateMainInput> {
        syn::parse2(tokens)
    }

    #[test]
    fn parses_migrations_dir_and_backends() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "apps/main/migrations",
            guard = GUARD,
            sql = (build_pool().await?, [User, Post]),
            offline = (open_db().await?, [CachedDoc]),
        })
        .expect("valid invocation");
        assert_eq!(input.sql.as_ref().unwrap().models.len(), 2);
        assert!(input.cql.is_none());
        assert_eq!(input.offline.as_ref().unwrap().models.len(), 1);
    }

    #[test]
    fn an_embedded_set_is_registered_before_the_app_s_models() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
            embedded = [&LEASE_MIGRATIONS, &acme_audit::AUDIT_MIGRATIONS],
        })
        .expect("valid invocation");
        assert_eq!(input.embedded.len(), 2);
        let tokens = build(input).to_string();
        let lease = tokens
            .find(". with_embedded (& LEASE_MIGRATIONS)")
            .expect("the first set is registered");
        let audit = tokens
            .find(". with_embedded (& acme_audit :: AUDIT_MIGRATIONS)")
            .expect("the second set is registered");
        let model = tokens
            .find(". sql_model :: < User > ()")
            .expect("the model");
        assert!(lease < audit && audit < model, "{tokens}");
    }

    #[test]
    fn a_named_list_of_embedded_sets_is_registered_in_its_order_before_the_models() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
            embedded = app::EMBEDDED_MIGRATIONS,
        })
        .expect("valid invocation");
        let tokens = build(input).to_string();
        let listed = tokens
            .find("app :: EMBEDDED_MIGRATIONS")
            .expect("the named list is registered");
        let model = tokens
            .find(". sql_model :: < User > ()")
            .expect("the model");
        assert!(listed < model, "{tokens}");
        assert!(tokens.contains(". with_embedded (__set)"), "{tokens}");
    }

    #[test]
    fn an_embedded_set_without_a_sql_backend_is_rejected() {
        let err = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            cql = (session, [Event]),
            embedded = [&LEASE_MIGRATIONS],
        })
        .expect_err("embedded needs the SQL wire");
        assert!(err.to_string().contains("needs `sql = "), "{err}");
    }

    #[test]
    fn no_embedded_key_registers_nothing_embedded() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
        })
        .expect("valid invocation");
        assert!(!build(input).to_string().contains("with_embedded"));
    }

    #[test]
    fn requires_migrations_dir() {
        let err =
            parse(quote! { app = "main", sql = (p, [User]) }).expect_err("migrations_dir required");
        assert!(err.to_string().contains("migrations_dir"));
    }

    #[test]
    fn requires_at_least_one_backend() {
        let err = parse(quote! { app = "main", migrations_dir = "m", guard = GUARD })
            .expect_err("a backend is required");
        assert!(err.to_string().contains("at least one backend"));
    }

    #[test]
    fn a_missing_guard_is_rejected() {
        let err = parse(quote! { app = "main", migrations_dir = "m", sql = (pool, [User]) })
            .expect_err("the guard is required");
        assert!(
            err.to_string().contains("requires `guard = <expr>`"),
            "{err}"
        );
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err =
            parse(quote! { app = "main", migrations_dir = "m", guard = GUARD, redis = (p, [X]) })
                .expect_err("unknown key");
        assert!(err.to_string().contains("unknown `migrate_main!` key"));
    }

    #[test]
    fn build_emits_a_main_dispatching_the_subcommands() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
        })
        .unwrap();
        let tokens = build(input).to_string();
        assert!(tokens.contains("fn main"));
        assert!(
            tokens.contains("ObservabilityBuilder :: boot (\"main\")"),
            "{tokens}"
        );
        assert!(tokens.contains("run_main"), "{tokens}");
        assert!(tokens.contains("check_sql"));
        assert!(tokens.contains("apply_sql"));
        assert!(tokens.contains("status_sql"));
        assert!(tokens.contains("MigrateRunner"));
    }

    #[test]
    fn verify_replays_the_sql_wire_against_the_base_after_the_word() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
        })
        .unwrap();
        let tokens = build(input).to_string();
        assert!(
            tokens.contains("MigrateCommand :: Verify { base : __base }"),
            "{tokens}"
        );
        assert!(
            tokens.contains("Box :: pin (__runner . verify_sql (& __conn , & __base)) ;"),
            "{tokens}"
        );
    }

    #[test]
    fn verify_without_a_sql_backend_is_refused_not_passed() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            cql = (session, [Event]),
        })
        .unwrap();
        let tokens = build(input).to_string();
        assert!(!tokens.contains("verify_sql"), "{tokens}");
        assert!(tokens.contains("MigrationError :: Unverified"), "{tokens}");
    }

    #[test]
    fn status_prints_each_server_wire_s_ledger_and_the_offline_count() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
            cql = (session, [Event]),
            offline = (db, [Cached]),
        })
        .unwrap();
        let tokens = build(input).to_string();
        assert!(
            tokens.contains("__runner . status_sql (& __conn)"),
            "{tokens}"
        );
        assert!(
            tokens.contains("__runner . status_cql (& __conn)"),
            "{tokens}"
        );
        assert!(
            tokens.contains("__runner . applied_offline (& __conn)"),
            "{tokens}"
        );
        assert!(!tokens.contains("applied_sql"), "{tokens}");
    }

    #[test]
    fn resolve_reads_who_from_the_environment_and_asks_each_server_wire_in_turn() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
            cql = (session, [Event]),
        })
        .unwrap();
        let tokens = build(input).to_string();
        assert!(
            tokens.contains(
                "MigrateCommand :: Resolve { name : __name , act : __act , reason : __reason }"
            ),
            "{tokens}"
        );
        assert!(
            tokens.contains("migration :: Resolution :: new (& __reason) ?"),
            "{tokens}"
        );
        let sql = tokens
            .find("Box :: pin (__runner . resolve_sql (& __conn , & __name , __act , & __resolution)) ;")
            .expect("the sql wire settles it");
        let cql = tokens
            .find("Box :: pin (__runner . resolve_cql (& __conn , & __name , __act , & __resolution)) ;")
            .expect("or the cql wire");
        assert!(sql < cql, "{tokens}");
        assert!(
            tokens.contains("MigrationError :: NotFound (__name)"),
            "{tokens}"
        );
    }

    #[test]
    fn the_binary_hands_every_word_after_its_name_to_run_main() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
        })
        .unwrap();
        let tokens = build(input).to_string();
        assert!(
            tokens.contains("run_main (__args . get (1 ..) . unwrap_or_default ()"),
            "{tokens}"
        );
    }

    #[test]
    fn the_migrator_records_the_environment_s_own_name() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
        })
        .unwrap();
        let tokens = build(input).to_string();
        assert!(
            tokens.contains("app . config () . environment_name ()"),
            "{tokens}"
        );
        assert!(!tokens.contains("__env . as_str ()"), "{tokens}");
    }

    #[test]
    fn telemetry_starts_and_flushes_inside_the_runtime() {
        let input = parse(quote! {
            app = "main",
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
        })
        .unwrap();
        let tokens = build(input).to_string();
        let runtime = tokens
            .find("runtime :: Runtime :: new ()")
            .expect("a runtime");
        let entered = tokens
            .find("__rt . enter ()")
            .expect("the runtime is entered");
        let boot = tokens
            .find("ObservabilityBuilder :: boot")
            .expect("telemetry boots");
        let flushed = tokens
            .find("drop (__telemetry)")
            .expect("telemetry is flushed");
        let left = tokens
            .find("drop (__entered)")
            .expect("the runtime is left");
        assert!(
            runtime < entered && entered < boot && flushed < left,
            "{tokens}"
        );
    }

    #[test]
    fn an_app_is_required_so_no_migrator_runs_without_telemetry() {
        let err = parse(quote! { migrations_dir = "m", guard = GUARD, sql = (pool, [User]) })
            .expect_err("the app is required");
        assert!(err.to_string().contains("requires `app"), "{err}");
    }

    #[test]
    fn a_telemetry_customisation_is_applied_before_init() {
        let input = parse(quote! {
            app = "main",
            telemetry = |builder| builder.with_directives(["sqlx=warn"]),
            migrations_dir = "m",
            guard = GUARD,
            sql = (pool, [User]),
        })
        .unwrap();
        let tokens = build(input).to_string();
        assert!(tokens.contains("with_directives"), "{tokens}");
        assert!(tokens.contains(". init ()"), "{tokens}");
    }
}
