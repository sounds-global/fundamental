//! Per-derive implementations and the crate-path resolution they share.

pub mod canonical_text;
pub(crate) mod case;
pub mod connection;
pub mod consumer;
pub(crate) mod entity_name;
pub mod error;
pub mod error_override;
pub mod identifier;
pub mod message;
pub mod message_set;
pub(crate) mod messages;
pub mod model;
pub mod namespace;
pub(crate) mod public_segment;
pub mod queue;
pub mod rpc_set;
pub mod schema;
pub mod scope;
pub mod stream;
pub(crate) mod template;
pub mod validate;
pub mod variant;

use proc_macro_crate::{Error as CrateResolveError, FoundCrate, crate_name};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// The `fundamental_core` root, resolved against the invoking crate: itself, a direct dependency, or the umbrella.
pub(crate) fn core_path() -> TokenStream {
    match crate_name("fundamental_core") {
        Ok(FoundCrate::Itself) => quote!(crate),
        Ok(FoundCrate::Name(n)) => {
            let ident = format_ident!("{n}");
            quote!(::#ident)
        }
        Err(_) => match crate_name("fundamental") {
            Ok(FoundCrate::Name(n)) => {
                let ident = format_ident!("{n}");
                quote!(::#ident::core)
            }
            Ok(FoundCrate::Itself) => quote!(crate::core),
            Err(_) => quote!(::fundamental::core),
        },
    }
}

/// A backend crate's root for generated code: `::fundamental_x` when depended on directly, else through the umbrella.
fn umbrella_routed_path(direct: &str, umbrella_mod: &str) -> TokenStream {
    match crate_name(direct) {
        // `Itself` covers the lib and its examples and tests; the lib resolves `::name` through `extern crate self as name;`.
        Ok(FoundCrate::Itself) => {
            let ident = format_ident!("{direct}");
            quote!(::#ident)
        }
        Ok(FoundCrate::Name(n)) => {
            let ident = format_ident!("{n}");
            quote!(::#ident)
        }
        Err(_) => match crate_name("fundamental") {
            Ok(FoundCrate::Name(n)) => {
                let ident = format_ident!("{n}");
                let module = format_ident!("{umbrella_mod}");
                quote!(::#ident::#module)
            }
            Ok(FoundCrate::Itself) => {
                let module = format_ident!("{umbrella_mod}");
                quote!(crate::#module)
            }
            Err(_) => {
                let module = format_ident!("{umbrella_mod}");
                quote!(::fundamental::#module)
            }
        },
    }
}

pub(crate) fn sql_path() -> TokenStream {
    umbrella_routed_path("fundamental_sql", "sql")
}

pub(crate) fn cql_path() -> TokenStream {
    umbrella_routed_path("fundamental_cql", "cql")
}

pub(crate) fn db_path() -> TokenStream {
    umbrella_routed_path("fundamental_db", "db")
}

pub(crate) fn authz_path() -> TokenStream {
    umbrella_routed_path("fundamental_authz", "authz")
}

pub(crate) fn encryption_path() -> TokenStream {
    umbrella_routed_path("fundamental_encryption", "encryption")
}

pub(crate) fn offline_path() -> TokenStream {
    umbrella_routed_path("fundamental_offline", "offline")
}

pub(crate) fn sync_path() -> TokenStream {
    umbrella_routed_path("fundamental_sync", "sync")
}

pub(crate) fn sync_server_path() -> TokenStream {
    umbrella_routed_path("fundamental_sync_server", "sync_server")
}

pub(crate) fn migrator_path() -> TokenStream {
    umbrella_routed_path("fundamental_db_migrator", "migrator")
}

pub(crate) fn observability_path() -> TokenStream {
    umbrella_routed_path("fundamental_observability", "observability")
}

pub(crate) fn inbox_path() -> TokenStream {
    umbrella_routed_path("fundamental_inbox", "inbox")
}

pub(crate) fn nats_path() -> TokenStream {
    umbrella_routed_path("fundamental_nats", "nats")
}

pub(crate) fn native_path() -> TokenStream {
    umbrella_routed_path("fundamental_native", "native")
}

pub(crate) fn ws_path() -> TokenStream {
    umbrella_routed_path("fundamental_ws", "ws")
}

pub(crate) fn server_path() -> TokenStream {
    umbrella_routed_path("fundamental_server", "server")
}

pub(crate) fn web_path() -> TokenStream {
    umbrella_routed_path("fundamental_web", "web")
}

/// `leptos` as the invoking crate names it; only a direct dependency counts, since the umbrella's features cannot be read.
pub(crate) fn leptos_path() -> Option<TokenStream> {
    match crate_name("leptos") {
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{name}");
            Some(quote!(::#ident))
        }
        Ok(FoundCrate::Itself) => Some(quote!(crate)),
        Err(_) => None,
    }
}

/// Whether the invoking crate is a web crate — see [`leptos_path`].
pub(crate) fn is_web_crate() -> bool {
    leptos_path().is_some()
}

pub(crate) fn schema_path() -> TokenStream {
    umbrella_routed_path("fundamental_schema", "schema")
}

pub(crate) fn seed_path() -> TokenStream {
    umbrella_routed_path("fundamental_seed", "seed")
}

pub(crate) fn command_path() -> TokenStream {
    umbrella_routed_path("fundamental_command", "command")
}

/// A backend crate path and an optional `#[cfg(feature)]` gate for a conditionally emitted impl; `None` when neither crate is a dependency.
pub(crate) fn backend_impl_target(
    direct: &str,
    umbrella_mod: &str,
) -> Option<(TokenStream, Option<TokenStream>)> {
    match crate_name(direct) {
        Ok(FoundCrate::Itself) => {
            let ident = format_ident!("{direct}");
            Some((quote!(::#ident), None))
        }
        Ok(FoundCrate::Name(n)) => {
            let ident = format_ident!("{n}");
            Some((quote!(::#ident), None))
        }
        Err(_) => match crate_name("fundamental") {
            Ok(FoundCrate::Name(n)) => {
                let ident = format_ident!("{n}");
                let module = format_ident!("{umbrella_mod}");
                Some((
                    quote!(::#ident::#module),
                    Some(quote!(#[cfg(feature = #umbrella_mod)])),
                ))
            }
            Ok(FoundCrate::Itself) => {
                let module = format_ident!("{umbrella_mod}");
                Some((
                    quote!(crate::#module),
                    Some(quote!(#[cfg(feature = #umbrella_mod)])),
                ))
            }
            // No manifest (a doctest): a gate on a feature that cannot exist would drop the impls silently.
            Err(CrateResolveError::CargoManifestDirNotSet) => {
                let module = format_ident!("{umbrella_mod}");
                Some((quote!(::fundamental::#module), None))
            }
            Err(_) => None,
        },
    }
}
