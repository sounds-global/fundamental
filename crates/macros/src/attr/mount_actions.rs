//! `mount_actions! { Greet, Farewell }`: a surface's actions, mounted on the server and called from the client, from one list.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, Path, Token, parse_macro_input};

use crate::derives::{core_path, is_web_crate, server_path, web_path};

struct MountActions {
    actions: Punctuated<Path, Token![,]>,
}

impl Parse for MountActions {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            actions: Punctuated::parse_terminated(input)?,
        })
    }
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as MountActions);
    let markers: Vec<&Path> = parsed.actions.iter().collect();

    let server = server_path();
    let core = core_path();

    let bounds = markers
        .iter()
        .map(|marker| quote!(#marker: #server::ServerAction<S>));
    let mounts = markers.iter().map(|marker| {
        quote!(
            let router = router.action::<#marker>()?;
        )
    });
    let descriptors = markers.iter().map(|marker| {
        quote! {
            #server::ActionDescriptor::new(
                <#marker as #core::action::Action>::METHOD,
                <#marker as #core::action::Action>::PATH,
                <#marker as #server::ActionMeta>::DOC,
                <#marker as #server::ActionMeta>::MIDDLEWARE,
                <#marker as #server::ActionMeta>::NO_SINGLE_OBJECT,
                #server::describe!(<#marker as #core::action::Action>::Input),
                #server::describe!(<#marker as #core::action::Action>::Output),
            )
        }
    });

    let server_arm = quote! {
        #[cfg(not(target_arch = "wasm32"))]
        /// Mounts every action listed in `mount_actions!` on `router`.
        pub fn register_actions<S>(
            router: #server::Router<S>,
        ) -> ::core::result::Result<#server::Router<S>, #server::ServerError>
        where
            S: ::core::marker::Send + ::core::marker::Sync + 'static,
            #(#bounds,)*
        {
            #(#mounts)*
            ::core::result::Result::Ok(router)
        }

        #[cfg(not(target_arch = "wasm32"))]
        /// Describes every action listed in `mount_actions!`.
        pub fn describe_actions() -> ::std::vec::Vec<#server::ActionDescriptor> {
            ::std::vec![#(#descriptors),*]
        }
    };

    // Built here from the marker, since the action may live in an API-only crate that emits no stub.
    let client_arm = if is_web_crate() {
        if let Err(error) = ensure_unique_hook_names(&markers) {
            return error.to_compile_error().into();
        }
        let web = web_path();
        let generated_groups = render_actions(&markers, &core, &web, 0);

        quote! {
            /// Typed client stubs for the actions this surface mounts.
            pub mod actions {
                #[allow(unused_imports)]
                use super::*;
                #generated_groups
            }
        }
    } else {
        TokenStream2::new()
    };

    quote! {
        #server_arm
        #client_arm
    }
    .into()
}

fn last_segment(path: &Path) -> String {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default()
}

fn snake_case(pascal: &str) -> String {
    let mut out = String::new();
    for (index, ch) in pascal.char_indices() {
        if ch.is_ascii_uppercase() {
            if index != 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// The hook name; the last module segment is dropped when it repeats the action name.
fn hook_name(path: &Path) -> String {
    let mut parts: Vec<String> = path
        .segments
        .iter()
        .map(|segment| snake_case(&segment.ident.to_string()))
        .collect();
    if parts.len() > 1 && parts[parts.len() - 2] == parts[parts.len() - 1] {
        parts.remove(parts.len() - 2);
    }
    format!("use_{}", parts.join("_"))
}

fn ensure_unique_hook_names(markers: &[&Path]) -> syn::Result<()> {
    let mut names: Vec<(String, &Path)> = Vec::new();
    for marker in markers {
        let name = hook_name(marker);
        if let Some((_, previous)) = names.iter().find(|(existing, _)| *existing == name) {
            return Err(syn::Error::new_spanned(
                *marker,
                format!(
                    "generated hook `{name}` collides for mounted action paths `{}` and `{}`",
                    path_name(previous),
                    path_name(marker),
                ),
            ));
        }
        names.push((name, marker));
    }
    Ok(())
}

fn path_name(path: &Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// The namespace tree; `depth` counts generated modules below `actions`, to reach the invocation scope.
fn render_actions(
    markers: &[&Path],
    core: &TokenStream2,
    web: &TokenStream2,
    depth: usize,
) -> TokenStream2 {
    let mut modules: Vec<(Ident, Vec<&Path>)> = Vec::new();
    let mut leaves = Vec::new();

    for marker in markers {
        if marker.segments.len() == depth + 1 {
            leaves.push(*marker);
            continue;
        }
        let module = marker.segments[depth].ident.clone();
        if let Some((_, grouped)) = modules.iter_mut().find(|(name, _)| *name == module) {
            grouped.push(*marker);
        } else {
            modules.push((module, vec![*marker]));
        }
    }

    let stubs = leaves.iter().map(|marker| {
        let name = format_ident!("{}", snake_case(&last_segment(marker)));
        let hook = format_ident!("{}", hook_name(marker));
        let mut marker_path = quote!();
        for _ in 0..=depth {
            marker_path.extend(quote!(super::));
        }
        marker_path.extend(quote!(#marker));
        quote! {
            /// Calls this action from the browser, running its server middleware chain.
            #[allow(non_upper_case_globals)]
            pub const #name: <#marker_path as #core::action::ClientAction>::Function<#web::WebActionTransport> =
                #marker_path::__fundamental_client::<#web::WebActionTransport>();

            /// Creates this action's typed handle for `<Query>` or `<Mutation>`.
            pub fn #hook<__FundamentalOperation>() -> #web::ActionHook<
                #marker_path,
                __FundamentalOperation,
            >
            where
                #marker_path: #core::action::ClientAction<Operation = __FundamentalOperation>,
                __FundamentalOperation: #core::action::ReactiveOperation,
            {
                #web::ActionHook::new()
            }
        }
    });
    let nested = modules.iter().map(|(module, grouped)| {
        let contents = render_actions(grouped, core, web, depth + 1);
        quote! {
            /// Client stubs for actions in this namespace.
            pub mod #module { #contents }
        }
    });

    quote!(#(#stubs)* #(#nested)*)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn snake_case_lowers_each_word_boundary() {
        assert_eq!(snake_case("Greet"), "greet");
        assert_eq!(snake_case("CreateUser"), "create_user");
        assert_eq!(snake_case("Me"), "me");
    }

    #[test]
    fn last_segment_takes_the_trailing_ident() {
        let path: Path = parse_quote!(domain::actions::Greet);
        assert_eq!(last_segment(&path), "Greet");
    }

    #[test]
    fn hook_names_include_the_namespace_and_remove_a_repeated_action_name() {
        assert_eq!(hook_name(&parse_quote!(Greet)), "use_greet");
        assert_eq!(hook_name(&parse_quote!(greet::Greet)), "use_greet");
        assert_eq!(hook_name(&parse_quote!(stats::Read)), "use_stats_read");
        assert_eq!(
            hook_name(&parse_quote!(users::profile::Get)),
            "use_users_profile_get"
        );
    }

    #[test]
    fn flattened_hook_name_collisions_name_both_actions() {
        let first: Path = parse_quote!(users::ProfileGet);
        let second: Path = parse_quote!(users_profile::Get);
        let error = ensure_unique_hook_names(&[&first, &second]).unwrap_err();
        assert!(error.to_string().contains("use_users_profile_get"));
        assert!(error.to_string().contains("users::ProfileGet"));
        assert!(error.to_string().contains("users_profile::Get"));
    }
}
