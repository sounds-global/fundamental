//! The state an `#[action]` handler is served over: one concrete type, or the handler's
//! own type parameter, bounded by the capabilities it reads.

use proc_macro2::{TokenStream as TokenStream2, TokenTree};
use quote::{ToTokens, quote};
use syn::{FnArg, GenericParam, Generics, Ident, ItemFn, ReturnType, Type, parse_quote};

use crate::attr::handler_state::{http_context_state, state_is_generic};

pub(crate) enum ActionState<'a> {
    /// `HttpContext<TeamApi>`: served over every state `TeamApi` is read out of.
    Concrete(&'a Type),
    /// `async fn browse<S: Reader>(ctx: HttpContext<S>, …)`: served over every `S` meeting the handler's bounds plus `Send + Sync + 'static`.
    Generic {
        state: &'a Ident,
        generics: Generics,
    },
}

impl<'a> ActionState<'a> {
    /// Reads the handler's state, refusing a handler generic over anything else, or input and output that name it.
    pub(crate) fn of(item: &'a ItemFn) -> syn::Result<Self> {
        let state = http_context_state(item)?;
        let generic = state_is_generic(item, state).then(|| match state {
            Type::Path(path) => path.path.get_ident(),
            _ => None,
        });
        let state_ident = generic.flatten();
        for param in &item.sig.generics.params {
            match param {
                GenericParam::Type(param) if Some(&param.ident) == state_ident => {}
                GenericParam::Type(param) => {
                    return Err(syn::Error::new_spanned(
                        param,
                        "an `#[action]` handler may be generic only over its state, \
                         as in `async fn browse<S: Reader>(ctx: HttpContext<S>, …)`",
                    ));
                }
                GenericParam::Lifetime(_) | GenericParam::Const(_) => {
                    return Err(syn::Error::new_spanned(
                        param,
                        "an `#[action]` handler takes no lifetime or const parameter: \
                         its input and output are owned, and its state is its one parameter",
                    ));
                }
            }
        }
        let Some(state) = state_ident else {
            return Ok(Self::Concrete(state));
        };
        for named in exchanged_types(item) {
            if names(named.to_token_stream(), state) {
                return Err(syn::Error::new_spanned(
                    named,
                    format!(
                        "an action's input and output cross to the browser, which has no state, \
                         so they cannot name the state `{state}`"
                    ),
                ));
            }
        }
        let mut generics = item.sig.generics.clone();
        generics.make_where_clause().predicates.push(parse_quote!(
            #state: ::core::marker::Send + ::core::marker::Sync + 'static
        ));
        Ok(Self::Generic { state, generics })
    }

    /// The handler as the route binds it: by name, or by turbofish for a generic state.
    pub(crate) fn handler(&self, handler: &Ident) -> TokenStream2 {
        match self {
            Self::Concrete(_) => quote!(#handler),
            Self::Generic { state, .. } => quote!(#handler::<#state>),
        }
    }
}

/// The handler's input and output types: what crosses to the browser.
fn exchanged_types(item: &ItemFn) -> impl Iterator<Item = &Type> {
    let input = item.sig.inputs.iter().nth(1).and_then(|arg| match arg {
        FnArg::Typed(pat) => Some(pat.ty.as_ref()),
        FnArg::Receiver(_) => None,
    });
    let output = match &item.sig.output {
        ReturnType::Type(_, ty) => Some(ty.as_ref()),
        ReturnType::Default => None,
    };
    input.into_iter().chain(output)
}

fn names(tokens: TokenStream2, ident: &Ident) -> bool {
    tokens.into_iter().any(|tree| match tree {
        TokenTree::Ident(found) => &found == ident,
        TokenTree::Group(group) => names(group.stream(), ident),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}
