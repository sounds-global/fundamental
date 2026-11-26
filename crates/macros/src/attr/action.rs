//! `#[action]`: an async HTTP handler bound to a method and path.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{
    Expr, FnArg, Ident, ItemFn, LitStr, ReturnType, Token, Type, Visibility, parse_macro_input,
};

use crate::attr::action_state::ActionState;
use crate::derives::{core_path, is_web_crate, server_path, web_path};

/// Entry point for `#[action(METHOD, "/path", middleware, …)]`.
pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ActionArgs);
    let item = parse_macro_input!(item as ItemFn);

    match build(args, item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// The parsed `#[action(...)]` arguments.
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
struct ActionArgs {
    method: Ident,
    path: LitStr,
    middlewares: Vec<Expr>,
    reactive: Option<ReactiveMode>,
    content_type: Option<LitStr>,
    bytes: bool,
    raw_input: bool,
    form: bool,
    accepts_form: bool,
    /// `no_single_object = "…"` — why the act names no one object.
    no_single_object: Option<LitStr>,
    /// `cross_origin = "…"` — the origin set this route answers, reading no credential.
    cross_origin: Option<LitStr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReactiveMode {
    Infer,
    Query,
    Mutation,
}

impl Parse for ActionArgs {
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let method: Ident = input.parse().map_err(|_| {
            input.error("`#[action]` expects a method, as in `#[action(POST, \"/users\")]`")
        })?;
        input.parse::<Token![,]>()?;
        let path: LitStr = input.parse()?;

        // Each argument after the path is a middleware, in order, unless it is a reserved marker.
        let mut middlewares = Vec::new();
        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            middlewares.push(input.parse::<Expr>()?);
        }

        let mut reactive = None;
        let mut bytes = false;
        let mut raw_input = false;
        let mut form = false;
        let mut accepts_form = false;
        let mut parsed_middlewares = Vec::new();
        for expr in middlewares {
            if let Some(mode) = parse_reactive_marker(&expr)? {
                once(reactive.replace(mode), &expr)?;
            } else if is_bytes_marker(&expr) {
                bytes = true;
            } else if is_raw_input_marker(&expr) {
                raw_input = true;
            } else if is_accepts_form_marker(&expr) {
                accepts_form = true;
            } else if is_form_marker(&expr) {
                form = true;
            } else {
                parsed_middlewares.push(expr);
            }
        }
        let mut middlewares = parsed_middlewares;

        let mut content_type = None;
        for expr in &middlewares {
            if let Some(lit) = content_type_argument(expr)? {
                content_type = Some(lit);
            }
        }
        middlewares.retain(|expr| !is_content_type_argument(expr));

        let mut no_single_object = None;
        for expr in &middlewares {
            if is_no_single_object_argument(expr) {
                no_single_object = Some(no_single_object_reason(expr)?);
            }
        }
        middlewares.retain(|expr| !is_no_single_object_argument(expr));

        let mut cross_origin = None;
        for expr in &middlewares {
            if is_keyword_argument(expr, "cross_origin") {
                cross_origin = Some(cross_origin_set(expr)?);
            }
        }
        middlewares.retain(|expr| !is_keyword_argument(expr, "cross_origin"));
        if let Some(bare) = middlewares
            .iter()
            .find(|expr| matches!(expr, Expr::Path(path) if path.path.is_ident("cross_origin")))
        {
            return Err(syn::Error::new_spanned(
                bare,
                "`cross_origin` names the origin set the route answers: `cross_origin = \"…\"`",
            ));
        }

        // Caught here: as a middleware it would fail later as an unresolved name, reading as a typo.
        if let Some(bare) = middlewares
            .iter()
            .find(|expr| matches!(expr, Expr::Path(path) if path.path.is_ident("no_single_object")))
        {
            return Err(syn::Error::new_spanned(
                bare,
                "`no_single_object` needs a reason: `no_single_object = \"…\"`, saying why the \
                 act names no one object",
            ));
        }

        if bytes && content_type.is_none() {
            return Err(input.error(
                "`bytes` requires a `content_type`, as in \
                 `#[action(GET, \"/tile\", content_type = \"application/octet-stream\", bytes)]`",
            ));
        }

        if raw_input && content_type.is_none() {
            return Err(input.error(
                "`raw_input` requires a `content_type`, as in \
                 `#[action(POST, \"/add-checkpoint\", content_type = \"text/plain; charset=utf-8\", raw_input)]`",
            ));
        }

        if form && (raw_input || bytes) {
            return Err(input.error("`form` cannot be combined with `raw_input` or `bytes`"));
        }

        if accepts_form && raw_input {
            return Err(input.error("`accepts_form` cannot be combined with `raw_input`"));
        }
        if accepts_form && content_type.is_some() && !bytes {
            return Err(input.error(
                "`accepts_form` answers JSON, or a file with `bytes`; a `content_type` \
                 alone would be ignored — add `bytes`, as in \
                 `#[action(POST, \"/export\", accepts_form, content_type = \"text/csv\", bytes)]`",
            ));
        }
        if accepts_form && form {
            return Err(input.error(
                "`accepts_form` already accepts a urlencoded body; naming `form` too \
                 declares two different shapes for one request",
            ));
        }

        Ok(Self {
            method,
            path,
            middlewares,
            reactive,
            content_type,
            bytes,
            raw_input,
            form,
            accepts_form,
            no_single_object,
            cross_origin,
        })
    }
}

fn parse_reactive_marker(expr: &Expr) -> syn::Result<Option<ReactiveMode>> {
    if matches!(expr, Expr::Path(path) if path.path.is_ident("reactive")) {
        return Ok(Some(ReactiveMode::Infer));
    }
    let Expr::Call(call) = expr else {
        return Ok(None);
    };
    if !matches!(call.func.as_ref(), Expr::Path(path) if path.path.is_ident("reactive")) {
        return Ok(None);
    }
    if call.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            call,
            "write `reactive(query)` or `reactive(mutation)`",
        ));
    }
    let argument = call.args.first().expect("one argument checked above");
    let Expr::Path(path) = argument else {
        return Err(syn::Error::new_spanned(
            argument,
            "the reactive role must be `query` or `mutation`",
        ));
    };
    let Some(role) = path.path.get_ident() else {
        return Err(syn::Error::new_spanned(
            argument,
            "the reactive role must be `query` or `mutation`",
        ));
    };
    match role.to_string().as_str() {
        "query" => Ok(Some(ReactiveMode::Query)),
        "mutation" => Ok(Some(ReactiveMode::Mutation)),
        _ => Err(syn::Error::new_spanned(
            argument,
            "the reactive role must be `query` or `mutation`",
        )),
    }
}

fn is_bytes_marker(expr: &Expr) -> bool {
    matches!(expr, Expr::Path(path) if path.path.is_ident("bytes"))
}

fn is_raw_input_marker(expr: &Expr) -> bool {
    matches!(expr, Expr::Path(path) if path.path.is_ident("raw_input"))
}

fn is_form_marker(expr: &Expr) -> bool {
    matches!(expr, Expr::Path(path) if path.path.is_ident("form"))
}

fn is_accepts_form_marker(expr: &Expr) -> bool {
    matches!(expr, Expr::Path(path) if path.path.is_ident("accepts_form"))
}

fn is_no_single_object_argument(expr: &Expr) -> bool {
    matches!(expr, Expr::Assign(assign)
        if matches!(assign.left.as_ref(), Expr::Path(path) if path.path.is_ident("no_single_object")))
}

/// The required, non-empty reason from a `no_single_object = "…"` argument.
fn no_single_object_reason(expr: &Expr) -> syn::Result<LitStr> {
    let Expr::Assign(assign) = expr else {
        return Err(syn::Error::new_spanned(
            expr,
            "expected `no_single_object = \"…\"`",
        ));
    };
    let Expr::Lit(lit) = assign.right.as_ref() else {
        return Err(syn::Error::new_spanned(
            &assign.right,
            "`no_single_object` takes a string literal saying why the act names no one object",
        ));
    };
    let syn::Lit::Str(reason) = &lit.lit else {
        return Err(syn::Error::new_spanned(
            &assign.right,
            "`no_single_object` takes a string literal saying why the act names no one object",
        ));
    };
    if reason.value().trim().len() < 20 {
        return Err(syn::Error::new_spanned(
            reason,
            "`no_single_object` needs a reason a reviewer can weigh, not a placeholder",
        ));
    }
    Ok(reason.clone())
}

fn is_keyword_argument(expr: &Expr, name: &str) -> bool {
    matches!(expr, Expr::Assign(assign)
        if matches!(assign.left.as_ref(), Expr::Path(path) if path.path.is_ident(name)))
}

/// The non-empty set name from a `cross_origin = "…"` argument.
fn cross_origin_set(expr: &Expr) -> syn::Result<LitStr> {
    let set = match expr {
        Expr::Assign(assign) => match assign.right.as_ref() {
            Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(set),
                ..
            }) => set.clone(),
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "`cross_origin` takes a string literal naming the origin set",
                ));
            }
        },
        other => {
            return Err(syn::Error::new_spanned(
                other,
                "expected `cross_origin = \"…\"`",
            ));
        }
    };
    if set.value().trim().is_empty() {
        return Err(syn::Error::new_spanned(
            &set,
            "`cross_origin` needs the name of an origin set the server configures",
        ));
    }
    Ok(set)
}

fn is_content_type_argument(expr: &Expr) -> bool {
    matches!(expr, Expr::Assign(assign)
        if matches!(assign.left.as_ref(), Expr::Path(path) if path.path.is_ident("content_type")))
}

/// The literal of a `content_type = "…"` argument; `None` for a middleware.
fn content_type_argument(expr: &Expr) -> syn::Result<Option<LitStr>> {
    if !is_content_type_argument(expr) {
        return Ok(None);
    }
    let Expr::Assign(assign) = expr else {
        return Ok(None);
    };
    match assign.right.as_ref() {
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(lit),
            ..
        }) => Ok(Some(lit.clone())),
        other => Err(syn::Error::new_spanned(
            other,
            "`content_type` expects a string literal, as in `content_type = \"application/jwt\"`",
        )),
    }
}

fn build(args: ActionArgs, item: ItemFn) -> syn::Result<TokenStream2> {
    if item.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            item.sig.fn_token,
            "an `#[action]` handler must be an `async fn`",
        ));
    }

    let method_variant = method_variant(&args.method)?;
    let state = ActionState::of(&item)?;
    let marker = marker_arm(&args, &item, &state)?;
    let server = server_arm(&args, &item, &state, &method_variant)?;

    // Emitted only for a web crate (a direct `leptos` dependency), decided here so no `feature` cfg reaches the generated code.
    let client = if is_web_crate() {
        client_arm(&args, &item)?
    } else {
        TokenStream2::new()
    };

    Ok(quote! {
        #marker
        #server
        #client
    })
}

/// The action's marker type and its `Action` and `ServerAction` impls; what `mount_actions!` lists.
fn marker_arm(args: &ActionArgs, item: &ItemFn, state: &ActionState) -> syn::Result<TokenStream2> {
    let core = core_path();
    let server = server_path();

    let handler = &item.sig.ident;
    let vis = &item.vis;
    let marker = format_ident!("{}", pascal_case(&handler.to_string()));
    let route = format_ident!("{handler}_route");
    let path = &args.path;
    let method_token = args.method.to_string().to_uppercase();

    let input = match action_input_type(item) {
        Some(ty) => quote!(#ty),
        None => quote!(()),
    };
    let output = match action_output_type(item) {
        Some(ty) => quote!(#ty),
        None => quote!(()),
    };
    let client_call = client_action_impl(&core, &marker, args, item)?;

    let doc = format!(
        "Typed handle for the `{} {}` action — list it in `mount_actions!` to serve it.",
        method_token,
        path.value()
    );

    let handler_doc = handler_doc(item);
    let middleware_names = args.middlewares.iter().map(middleware_name);
    let no_single_object_const: TokenStream2 = match &args.no_single_object {
        Some(reason) => quote! { ::core::option::Option::Some(#reason) },
        None => quote! { ::core::option::Option::None },
    };

    let path_fn = path_fn(&core, path, &marker)?;
    let server_action = server_action_impl(&server, &marker, &route, state);

    Ok(quote! {
        #[doc = #doc]
        #[allow(dead_code)]
        #vis struct #marker;

        #path_fn

        impl #core::action::Action for #marker {
            type Input = #input;
            type Output = #output;
            const METHOD: &'static str = #method_token;
            const PATH: &'static str = #path;
        }

        #client_call

        #[cfg(not(target_arch = "wasm32"))]
        #server_action

        #[cfg(not(target_arch = "wasm32"))]
        impl #server::ActionMeta for #marker {
            const DOC: &'static str = #handler_doc;
            const MIDDLEWARE: &'static [&'static str] = &[#(#middleware_names),*];
            const NO_SINGLE_OBJECT: ::core::option::Option<&'static str> = #no_single_object_const;
        }
    })
}

/// The marker's `ServerAction`, over a concrete or a generic state.
fn server_action_impl(
    server: &TokenStream2,
    marker: &Ident,
    route: &Ident,
    state: &ActionState,
) -> TokenStream2 {
    let state = match state {
        ActionState::Concrete(state) => state,
        ActionState::Generic { state, generics } => {
            let (impl_generics, _, where_clause) = generics.split_for_impl();
            return quote! {
                #[allow(clippy::multiple_bound_locations)]
                impl #impl_generics #server::ServerAction<#state> for #marker #where_clause {
                    fn route() -> #server::Route<#state> {
                        #route::<#state>()
                    }
                }
            };
        }
    };
    quote! {
        impl<__Outer> #server::ServerAction<__Outer> for #marker
        where
            #state: #server::FromRef<__Outer>
                + ::core::marker::Send
                + ::core::marker::Sync
                + 'static,
            __Outer: ::core::marker::Send + ::core::marker::Sync + 'static,
        {
            fn route() -> #server::Route<__Outer> {
                #route().project::<__Outer>()
            }
        }
    }
}

/// The client role: an explicit `reactive` override, else GET is a query and the rest mutations.
fn action_operation(core: &TokenStream2, args: &ActionArgs) -> TokenStream2 {
    match args.reactive {
        Some(ReactiveMode::Query) => quote!(#core::action::QueryOperation),
        Some(ReactiveMode::Mutation) => quote!(#core::action::MutationOperation),
        None | Some(ReactiveMode::Infer) if args.method.to_string().eq_ignore_ascii_case("GET") => {
            quote!(#core::action::QueryOperation)
        }
        None | Some(ReactiveMode::Infer) => quote!(#core::action::MutationOperation),
    }
}

fn once(previous: Option<ReactiveMode>, expr: &Expr) -> syn::Result<()> {
    match previous {
        Some(_) => Err(syn::Error::new_spanned(
            expr,
            "`reactive` may only be specified once",
        )),
        None => Ok(()),
    }
}

/// A transport-generic callable on the marker, so `mount_actions!` needs neither the handler's module nor a macro from this crate.
fn client_action_impl(
    core: &TokenStream2,
    marker: &Ident,
    args: &ActionArgs,
    item: &ItemFn,
) -> syn::Result<TokenStream2> {
    let method = args.method.to_string().to_uppercase();
    let path = &args.path;
    let operation = action_operation(core, args);
    let success = action_success_type(item)
        .or_else(|| action_output_type(item))
        .map_or_else(|| quote!(()), |success| quote!(#success));
    let has_input = action_input_type(item);
    let call_input = has_input.map_or_else(|| quote!(()), |input| quote!(#input));

    let (function_type, function_body) = client_function(core, &method, path, &success, has_input);

    Ok(quote! {
        impl #core::action::ClientAction for #marker {
            type Operation = #operation;
            type Function<T: #core::action::ActionTransport> = #function_type;
            type Success = #success;

            fn call<T: #core::action::ActionTransport>(input: Self::Input)
                -> T::Future<Self::Success>
            {
                T::call::<#call_input, Self::Success>(
                    #method,
                    ::std::borrow::Cow::Borrowed(#path),
                    input,
                )
            }

            fn call_at<T: #core::action::ActionTransport>(
                path: #core::action::ActionPath<Self>,
                input: Self::Input,
            ) -> T::Future<Self::Success>
            where
                Self: Sized,
            {
                T::call::<#call_input, Self::Success>(
                    #method,
                    ::std::borrow::Cow::Owned(path.into_string()),
                    input,
                )
            }
        }

        impl #marker {
            #[doc(hidden)]
            pub const fn __fundamental_client<T: #core::action::ActionTransport>()
                -> <Self as #core::action::ClientAction>::Function<T>
            {
                #function_body
            }
        }
    })
}

/// The `actions::name` function's type and body for `mount_actions!`.
fn client_function(
    core: &TokenStream2,
    method: &str,
    path: &LitStr,
    success: &TokenStream2,
    has_input: Option<&Type>,
) -> (TokenStream2, TokenStream2) {
    if let Some(input) = has_input {
        (
            quote!(fn(#input) -> T::Future<#success>),
            quote! {
                fn __fundamental_call<T: #core::action::ActionTransport>(
                    input: #input,
                ) -> T::Future<#success> {
                    T::call::<#input, #success>(#method, ::std::borrow::Cow::Borrowed(#path), input)
                }
                __fundamental_call::<T>
            },
        )
    } else {
        (
            quote!(fn() -> T::Future<#success>),
            quote! {
                fn __fundamental_call<T: #core::action::ActionTransport>() -> T::Future<#success> {
                    T::call::<(), #success>(#method, ::std::borrow::Cow::Borrowed(#path), ())
                }
                __fundamental_call::<T>
            },
        )
    }
}

fn action_success_type(item: &ItemFn) -> Option<&Type> {
    let ReturnType::Type(_, ty) = &item.sig.output else {
        return None;
    };
    let Type::Path(path) = ty.as_ref() else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

/// The marker's `path(...)`: a single-segment capture takes `impl PathSegment`, which no raw id implements.
fn path_fn(core: &TokenStream2, path: &LitStr, marker: &Ident) -> syn::Result<TokenStream2> {
    let template = path.value();
    let mut format = String::new();
    let mut args = Vec::new();
    let mut fills = Vec::new();
    for (at, segment) in template.split('/').enumerate() {
        if at > 0 {
            format.push('/');
        }
        let capture = segment
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'));
        let Some(capture) = capture else {
            if segment.contains(['{', '}']) {
                return Err(syn::Error::new_spanned(
                    path,
                    "a capture must be a whole path segment, as in `/teams/{team}`",
                ));
            }
            format.push_str(segment);
            continue;
        };
        let (name, spans) = match capture.strip_prefix('*') {
            Some(name) => (name, true),
            None => (capture, false),
        };
        let ident: Ident = syn::parse_str(name).map_err(|_| {
            syn::Error::new_spanned(path, format!("capture `{name}` is not a Rust identifier"))
        })?;
        format.push('{');
        format.push_str(name);
        format.push('}');
        match spans {
            true => {
                args.push(quote!(#ident: impl ::core::fmt::Display));
                fills.push(quote!(#ident = #ident));
            }
            false => {
                args.push(quote!(#ident: impl #core::address::PathSegment));
                fills.push(quote!(#ident = #core::address::PathSegment::to_segment(&#ident)));
            }
        }
    }
    let filled = match fills.is_empty() {
        true => quote!(::std::string::String::from(#template)),
        false => quote!(::std::format!(#format, #(#fills),*)),
    };
    let doc = format!(
        "`{template}` with each capture in its public form — what a form's `action` carries."
    );
    Ok(quote! {
        #[allow(dead_code)]
        impl #marker {
            #[doc = #doc]
            #[must_use]
            pub fn path(#(#args),*) -> #core::action::ActionPath<Self> {
                #core::action::ActionPath::filled(#filled)
            }
        }
    })
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

/// The `{fn}_route()` builder; the handler is nested inside it so the browser stub can take its name.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn server_arm(
    args: &ActionArgs,
    item: &ItemFn,
    state: &ActionState,
    method_variant: &Ident,
) -> syn::Result<TokenStream2> {
    let server = server_path();

    let handler = &item.sig.ident;
    let vis = &item.vis;
    let route = format_ident!("{handler}_route");
    let path = &args.path;

    // Set on the route, not pushed as a middleware: it describes the act.
    let object_declaration: TokenStream2 = match &args.no_single_object {
        Some(_) => quote! { .no_single_object() },
        None => TokenStream2::new(),
    };
    let origin_declaration: TokenStream2 = match &args.cross_origin {
        Some(set) => quote! { .cross_origin(#set) },
        None => TokenStream2::new(),
    };

    let chain: Vec<TokenStream2> = args
        .middlewares
        .iter()
        .map(|middleware| quote!(.middleware(#middleware)))
        .collect();

    let doc = format!(
        "Builds the `{} {}` route binding the `{handler}` handler, for a `Router`.",
        args.method.to_string().to_uppercase(),
        path.value()
    );

    // A handler without an input parameter gets the unit input, meeting `Route::new`'s two-argument bound.
    let named = state.handler(handler);
    let bound_handler = if item.sig.inputs.len() > 1 {
        named
    } else {
        quote!(|__ctx, _input: ()| #named(__ctx))
    };
    let (route_generics, route_state, where_clause) = match state {
        ActionState::Concrete(state) => (quote!(), quote!(#state), quote!()),
        ActionState::Generic { state, generics } => {
            let (impl_generics, _, where_clause) = generics.split_for_impl();
            (
                quote!(#impl_generics),
                quote!(#state),
                quote!(#where_clause),
            )
        }
    };

    let mut handler_item = item.clone();
    handler_item.vis = Visibility::Inherited;

    // A `content_type` route returns its output verbatim; `raw_input` also takes the body verbatim.
    let build_route = if let (true, true, Some(content_type)) =
        (args.accepts_form, args.bytes, &args.content_type)
    {
        quote! {
            #server::Route::action_bytes_body_or_form(
                #server::Method::#method_variant,
                #path,
                #content_type,
                #bound_handler,
            )
        }
    } else if args.accepts_form {
        quote! {
            #server::Route::action_body_or_form(
                #server::Method::#method_variant,
                #path,
                #bound_handler,
            )
        }
    } else if args.form {
        quote! {
            #server::Route::action_form(#server::Method::#method_variant, #path, #bound_handler)
        }
    } else {
        match (&args.content_type, args.bytes, args.raw_input) {
            (Some(content_type), true, true) => quote! {
                #server::Route::action_bytes_io(
                    #server::Method::#method_variant,
                    #path,
                    #content_type,
                    #bound_handler,
                )
            },
            (Some(content_type), false, true) => quote! {
                #server::Route::action_raw_input(
                    #server::Method::#method_variant,
                    #path,
                    #content_type,
                    #bound_handler,
                )
            },
            (Some(content_type), true, false) => quote! {
                #server::Route::action_bytes(
                    #server::Method::#method_variant,
                    #path,
                    #content_type,
                    #bound_handler,
                )
            },
            (Some(content_type), false, false) => quote! {
                #server::Route::action_raw(
                    #server::Method::#method_variant,
                    #path,
                    #content_type,
                    #bound_handler,
                )
            },
            (None, _, _) => quote! {
                #server::Route::action(#server::Method::#method_variant, #path, #bound_handler)
            },
        }
    };

    Ok(quote! {
        #[cfg(not(target_arch = "wasm32"))]
        #[doc = #doc]
        #[allow(clippy::multiple_bound_locations)]
        #vis fn #route #route_generics () -> #server::Route<#route_state> #where_clause {
            // Imported inside the `wasm32`-gated builder, since `HttpContext` is server-only.
            #[allow(unused_imports)]
            use #server::HttpContext;
            #handler_item
            let route = #build_route;
            let route = route #object_declaration #origin_declaration;
            route #(#chain)*
        }
    })
}

/// The client stub: a typed `async fn` of the same name that requests the action's route, so the server runs the chain.
fn client_arm(args: &ActionArgs, item: &ItemFn) -> syn::Result<TokenStream2> {
    let web = web_path();

    let handler = &item.sig.ident;
    let output = &item.sig.output;
    let path = &args.path;
    let method_token = args.method.to_string().to_uppercase();

    // The input binds to a fixed `input` name so the stub never depends on the handler's pattern.
    let input_type = action_input_type(item);
    let (input_param, input_ref) = match input_type {
        Some(ty) => (quote!(input: #ty), quote!(&input)),
        None => (quote!(), quote!(&())),
    };

    let doc = format!(
        "Calls the `{} {}` action from the browser, running its server middleware chain.",
        method_token,
        path.value()
    );

    Ok(quote! {
        #[doc = #doc]
        pub async fn #handler(#input_param) #output {
            #web::call_action(#method_token, #path, #input_ref).await
        }
    })
}

/// The handler's `///` docs joined and trimmed; empty when it has none.
fn handler_doc(item: &ItemFn) -> String {
    let lines: Vec<String> = item
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            syn::Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit),
                    ..
                }) => Some(lit.value().trim().to_string()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    lines.join("\n").trim().to_string()
}

/// A middleware's display name for `ActionMeta::MIDDLEWARE`: a path's last segment, else its tokens.
fn middleware_name(expr: &Expr) -> String {
    match expr {
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default(),
        other => quote!(#other)
            .to_string()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect(),
    }
}

/// The handler's second parameter type, if any.
fn action_input_type(item: &ItemFn) -> Option<&Type> {
    match item.sig.inputs.iter().nth(1) {
        Some(FnArg::Typed(pat)) => Some(pat.ty.as_ref()),
        _ => None,
    }
}

/// The handler's return type, if any.
fn action_output_type(item: &ItemFn) -> Option<&Type> {
    match &item.sig.output {
        ReturnType::Type(_, ty) => Some(ty.as_ref()),
        ReturnType::Default => None,
    }
}

fn method_variant(method: &Ident) -> syn::Result<Ident> {
    let variant = match method.to_string().to_uppercase().as_str() {
        "GET" => "Get",
        "POST" => "Post",
        "PUT" => "Put",
        "PATCH" => "Patch",
        "DELETE" => "Delete",
        _ => {
            return Err(syn::Error::new_spanned(
                method,
                "unsupported method; expected one of GET, POST, PUT, PATCH, DELETE",
            ));
        }
    };
    Ok(format_ident!("{variant}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn maps_each_method_case_insensitively() {
        for (input, expected) in [
            ("get", "Get"),
            ("POST", "Post"),
            ("Put", "Put"),
            ("patch", "Patch"),
            ("DELETE", "Delete"),
        ] {
            let ident = Ident::new(input, proc_macro2::Span::call_site());
            assert_eq!(method_variant(&ident).expect("known method"), expected);
        }
    }

    #[test]
    fn rejects_an_unknown_method() {
        let ident = Ident::new("CONNECT", proc_macro2::Span::call_site());
        let err = method_variant(&ident).expect_err("unsupported method");
        assert!(err.to_string().contains("unsupported method"));
    }

    #[test]
    fn omitting_the_input_param_supplies_a_unit_adapter() {
        let args = ActionArgs {
            method: Ident::new("GET", proc_macro2::Span::call_site()),
            path: parse_quote!("/me"),
            middlewares: Vec::new(),
            reactive: None,
            content_type: None,
            bytes: false,
            raw_input: false,
            form: false,
            accepts_form: false,
            no_single_object: None,
            cross_origin: None,
        };
        let item: ItemFn = parse_quote! {
            async fn me(ctx: HttpContext<AppState>) -> Result<Me, AnyError> { Ok(Me) }
        };
        let expanded = build(args, item).expect("expands").to_string();
        assert!(expanded.contains("fn me_route"));
        assert!(expanded.contains("_input : ()"));
    }

    #[test]
    fn keeping_the_input_param_binds_the_handler_directly() {
        let args = ActionArgs {
            method: Ident::new("POST", proc_macro2::Span::call_site()),
            path: parse_quote!("/echo"),
            middlewares: Vec::new(),
            reactive: None,
            content_type: None,
            bytes: false,
            raw_input: false,
            form: false,
            accepts_form: false,
            no_single_object: None,
            cross_origin: None,
        };
        let item: ItemFn = parse_quote! {
            async fn echo(ctx: HttpContext<AppState>, input: In) -> Result<Out, AnyError> { Ok(Out) }
        };
        let expanded = build(args, item).expect("expands").to_string();
        assert!(expanded.contains("fn echo_route"));
        assert!(!expanded.contains("_input : ()"));
    }

    fn get_at(path: &str) -> ActionArgs {
        ActionArgs {
            method: Ident::new("GET", proc_macro2::Span::call_site()),
            path: LitStr::new(path, proc_macro2::Span::call_site()),
            middlewares: Vec::new(),
            reactive: None,
            content_type: None,
            bytes: false,
            raw_input: false,
            form: false,
            accepts_form: false,
            no_single_object: None,
            cross_origin: None,
        }
    }

    #[test]
    fn a_concrete_state_action_is_served_over_every_state_it_is_read_out_of() {
        let item: ItemFn = parse_quote! {
            async fn me(ctx: HttpContext<TeamApi>) -> Result<Me, AnyError> { Ok(Me) }
        };
        let expanded = build(get_at("/me"), item).expect("expands").to_string();
        assert!(expanded.contains("impl < __Outer >"));
        assert!(expanded.contains("ServerAction < __Outer > for Me"));
        assert!(expanded.contains("FromRef < __Outer >"));
        assert!(expanded.contains("me_route () . project :: < __Outer > ()"));
    }

    #[test]
    fn a_generic_state_action_is_served_over_every_state_meeting_its_bounds() {
        let item: ItemFn = parse_quote! {
            async fn browse<S: Reader>(ctx: HttpContext<S>, query: Query) -> Result<Page, AnyError>
            where
                S: Clone,
            {
                Ok(Page)
            }
        };
        let expanded = build(get_at("/articles"), item)
            .expect("expands")
            .to_string();
        assert!(expanded.contains("impl < S : Reader >"), "{expanded}");
        assert!(expanded.contains("ServerAction < S > for Browse where S : Clone"));
        assert!(expanded.contains("browse_route :: < S > ()"));
        assert!(expanded.contains("fn browse_route < S : Reader > ()"));
        assert!(expanded.contains("Route :: action (") && expanded.contains("browse :: < S >"));
        assert!(!expanded.contains("project"));
        assert!(expanded.contains("struct Browse ;"));
        assert!(expanded.contains("for Browse { type Input = Query ;"));
    }

    #[test]
    fn a_generic_state_action_without_an_input_names_its_handler_for_the_state() {
        let item: ItemFn = parse_quote! {
            async fn count<S: Reader>(ctx: HttpContext<S>) -> Result<u64, AnyError> { Ok(0) }
        };
        let expanded = build(get_at("/count"), item).expect("expands").to_string();
        assert!(expanded.contains("count :: < S > (__ctx)"), "{expanded}");
    }

    fn refusal(item: ItemFn) -> String {
        build(get_at("/x"), item).expect_err("refused").to_string()
    }

    #[test]
    fn a_type_parameter_other_than_the_state_is_refused() {
        let message = refusal(parse_quote! {
            async fn x<S: Reader, T>(ctx: HttpContext<S>) -> Result<(), AnyError> { Ok(()) }
        });
        assert!(message.contains("only over its state"), "{message}");
        let message = refusal(parse_quote! {
            async fn x<T>(ctx: HttpContext<AppState>) -> Result<(), AnyError> { Ok(()) }
        });
        assert!(message.contains("only over its state"), "{message}");
    }

    #[test]
    fn a_lifetime_or_const_parameter_is_refused() {
        let message = refusal(parse_quote! {
            async fn x<'a, S: Reader>(ctx: HttpContext<S>) -> Result<(), AnyError> { Ok(()) }
        });
        assert!(message.contains("lifetime or const"), "{message}");
        let message = refusal(parse_quote! {
            async fn x<S: Reader, const N: usize>(ctx: HttpContext<S>) -> Result<(), AnyError> { Ok(()) }
        });
        assert!(message.contains("lifetime or const"), "{message}");
    }

    #[test]
    fn an_input_or_output_naming_the_state_is_refused() {
        let message = refusal(parse_quote! {
            async fn x<S: Reader>(ctx: HttpContext<S>, input: Vec<S>) -> Result<(), AnyError> { Ok(()) }
        });
        assert!(message.contains("cannot name the state `S`"), "{message}");
        let message = refusal(parse_quote! {
            async fn x<S: Reader>(ctx: HttpContext<S>) -> Result<Page<S>, AnyError> { Ok(todo!()) }
        });
        assert!(message.contains("cannot name the state `S`"), "{message}");
    }

    /// Expands only the client stub; `build` omits it in this non-web crate.
    fn client(args: ActionArgs, item: &ItemFn) -> syn::Result<String> {
        client_arm(&args, item).map(|tokens| tokens.to_string())
    }

    #[test]
    fn the_server_route_builder_is_gated_off_the_browser_and_nests_the_handler() {
        let args = ActionArgs {
            method: Ident::new("POST", proc_macro2::Span::call_site()),
            path: parse_quote!("/users"),
            middlewares: vec![parse_quote!(auth)],
            reactive: None,
            content_type: None,
            bytes: false,
            raw_input: false,
            form: false,
            accepts_form: false,
            no_single_object: None,
            cross_origin: None,
        };
        let item: ItemFn = parse_quote! {
            async fn create_user(ctx: HttpContext<AppState>, input: NewUser) -> Result<User, AnyError> {
                Ok(User)
            }
        };
        let expanded = server_arm(
            &args,
            &item,
            &ActionState::of(&item).unwrap(),
            &method_variant(&args.method).unwrap(),
        )
        .expect("expands")
        .to_string();
        // The handler lives inside the gated builder, so the stub can take its name.
        assert!(expanded.contains("cfg (not (target_arch = \"wasm32\"))"));
        assert!(expanded.contains("fn create_user_route"));
        assert!(expanded.contains("middleware (auth)"));
    }

    #[test]
    fn the_client_arm_is_omitted_for_a_non_web_crate() {
        let args = ActionArgs {
            method: Ident::new("POST", proc_macro2::Span::call_site()),
            path: parse_quote!("/users"),
            middlewares: Vec::new(),
            reactive: Some(ReactiveMode::Infer),
            content_type: None,
            bytes: false,
            raw_input: false,
            form: false,
            accepts_form: false,
            no_single_object: None,
            cross_origin: None,
        };
        let item: ItemFn = parse_quote! {
            async fn create_user(ctx: HttpContext<AppState>, input: NewUser) -> Result<User, AnyError> {
                Ok(User)
            }
        };
        assert!(!is_web_crate());
        let expanded = build(args, item).expect("expands").to_string();
        assert!(expanded.contains("fn create_user_route"));
        assert!(!expanded.contains("call_action"));
        assert!(!expanded.contains("create_user_action"));
    }

    #[test]
    fn the_client_stub_calls_the_transport_with_the_method_and_input_type() {
        let args = ActionArgs {
            method: Ident::new("POST", proc_macro2::Span::call_site()),
            path: parse_quote!("/users"),
            middlewares: Vec::new(),
            reactive: None,
            content_type: None,
            bytes: false,
            raw_input: false,
            form: false,
            accepts_form: false,
            no_single_object: None,
            cross_origin: None,
        };
        let item: ItemFn = parse_quote! {
            async fn create_user(ctx: HttpContext<AppState>, input: NewUser) -> Result<User, AnyError> {
                Ok(User)
            }
        };
        let expanded = client(args, &item).expect("expands");
        assert!(expanded.contains("call_action"));
        assert!(expanded.contains("\"POST\""));
        assert!(expanded.contains("input : NewUser"));
        assert!(expanded.contains("& input"));
    }

    #[test]
    fn the_client_stub_sends_unit_when_the_handler_takes_no_input() {
        let args = ActionArgs {
            method: Ident::new("GET", proc_macro2::Span::call_site()),
            path: parse_quote!("/me"),
            middlewares: Vec::new(),
            reactive: None,
            content_type: None,
            bytes: false,
            raw_input: false,
            form: false,
            accepts_form: false,
            no_single_object: None,
            cross_origin: None,
        };
        let item: ItemFn = parse_quote! {
            async fn me(ctx: HttpContext<AppState>) -> Result<Me, AnyError> { Ok(Me) }
        };
        let expanded = client(args, &item).expect("expands");
        assert!(expanded.contains("async fn me ()"));
        assert!(expanded.contains("& ()"));
        assert!(expanded.contains("\"GET\""));
    }

    #[test]
    fn the_reactive_marker_infers_mutation_metadata_and_is_not_a_middleware() {
        let attr: ActionArgs = syn::parse_quote!(POST, "/users", reactive, auth);
        assert_eq!(attr.reactive, Some(ReactiveMode::Infer));
        assert_eq!(attr.middlewares.len(), 1);

        let item: ItemFn = parse_quote! {
            async fn create_user(ctx: HttpContext<AppState>, input: NewUser) -> Result<User, AnyError> {
                Ok(User)
            }
        };
        let marker = marker_arm(&attr, &item, &ActionState::of(&item).unwrap()).expect("expands");
        assert!(
            marker
                .to_string()
                .contains("type Operation = :: fundamental_core :: action :: MutationOperation")
        );
        let client = client(attr, &item).expect("expands");
        assert!(client.contains("async fn create_user"));
        assert!(!client.contains("create_user_action"));
        assert!(!client.contains("new_local"));
        assert!(!client.contains("middleware (reactive)"));
    }

    #[test]
    fn an_unmarked_action_gets_the_method_default_operation() {
        let attr: ActionArgs = syn::parse_quote!(POST, "/users", auth);
        assert_eq!(attr.reactive, None);
        let item: ItemFn = parse_quote! {
            async fn create_user(ctx: HttpContext<AppState>, input: NewUser) -> Result<User, AnyError> {
                Ok(User)
            }
        };
        let marker = marker_arm(&attr, &item, &ActionState::of(&item).unwrap()).expect("expands");
        let client = client(attr, &item).expect("expands");
        assert!(client.contains("async fn create_user"));
        assert!(!client.contains("create_user_action"));
        assert!(!client.contains("new_local"));
        assert!(
            marker
                .to_string()
                .contains("type Operation = :: fundamental_core :: action :: MutationOperation")
        );
    }

    #[test]
    fn reactive_role_defaults_to_get_query_and_can_override_post() {
        let get: ActionArgs = syn::parse_quote!(GET, "/stats");
        let post: ActionArgs = syn::parse_quote!(POST, "/users");
        let get_override: ActionArgs = syn::parse_quote!(GET, "/rebuild", reactive(mutation));
        let query_override: ActionArgs = syn::parse_quote!(POST, "/search", reactive(query));

        let get_operation = action_operation(&core_path(), &get).to_string();
        let post_operation = action_operation(&core_path(), &post).to_string();
        let get_override_operation = action_operation(&core_path(), &get_override).to_string();
        let query_override_operation = action_operation(&core_path(), &query_override).to_string();

        assert!(get_operation.ends_with("QueryOperation"));
        assert!(post_operation.ends_with("MutationOperation"));
        assert!(get_override_operation.ends_with("MutationOperation"));
        assert!(query_override_operation.ends_with("QueryOperation"));
    }

    #[test]
    fn an_action_without_a_return_type_gets_a_unit_hook_result() {
        let attr: ActionArgs = syn::parse_quote!(POST, "/ping");
        let item: ItemFn = parse_quote! {
            async fn ping(ctx: HttpContext<AppState>, input: Ping) { }
        };
        let marker = marker_arm(&attr, &item, &ActionState::of(&item).unwrap()).expect("expands");
        assert!(marker.to_string().contains("type Success = ()"));
    }

    #[test]
    fn the_bytes_marker_binds_a_binary_route_and_is_not_a_middleware() {
        let attr: ActionArgs = syn::parse_quote!(
            GET,
            "/tile/{*rest}",
            content_type = "application/octet-stream",
            bytes
        );
        assert!(attr.bytes);
        assert_eq!(attr.middlewares.len(), 0);

        let item: ItemFn = parse_quote! {
            async fn tile(ctx: HttpContext<AppState>) -> Result<Vec<u8>, AnyError> { Ok(Vec::new()) }
        };
        let method = method_variant(&attr.method).unwrap();
        let expanded = server_arm(&attr, &item, &ActionState::of(&item).unwrap(), &method)
            .expect("expands")
            .to_string();
        assert!(expanded.contains("Route :: action_bytes"), "{expanded}");
        assert!(!expanded.contains("action_raw"));
        assert!(expanded.contains("\"application/octet-stream\""));
    }

    #[test]
    fn a_string_content_type_still_binds_the_raw_route() {
        let attr: ActionArgs = syn::parse_quote!(GET, "/jwt", content_type = "application/jwt");
        assert!(!attr.bytes);
        let item: ItemFn = parse_quote! {
            async fn jwt(ctx: HttpContext<AppState>) -> Result<String, AnyError> { Ok(String::new()) }
        };
        let method = method_variant(&attr.method).unwrap();
        let expanded = server_arm(&attr, &item, &ActionState::of(&item).unwrap(), &method)
            .expect("expands")
            .to_string();
        assert!(expanded.contains("Route :: action_raw"), "{expanded}");
        assert!(!expanded.contains("action_bytes"));
    }

    #[test]
    fn the_raw_input_marker_binds_a_raw_body_route_under_its_content_type() {
        let attr: ActionArgs = syn::parse_quote!(
            POST,
            "/add-checkpoint",
            content_type = "text/plain; charset=utf-8",
            raw_input
        );
        assert!(attr.raw_input);
        assert_eq!(attr.middlewares.len(), 0);

        let item: ItemFn = parse_quote! {
            async fn add_checkpoint(ctx: HttpContext<AppState>, body: String) -> Result<String, AnyError> {
                Ok(body)
            }
        };
        let method = method_variant(&attr.method).unwrap();
        let expanded = server_arm(&attr, &item, &ActionState::of(&item).unwrap(), &method)
            .expect("expands")
            .to_string();
        assert!(expanded.contains("Route :: action_raw_input"), "{expanded}");
        assert!(!expanded.contains("action_raw ("));
        assert!(!expanded.contains("action_bytes"));
    }

    #[test]
    fn the_form_marker_binds_a_form_body_route_and_is_not_a_middleware() {
        let attr: ActionArgs = syn::parse_quote!(POST, "/revoke", form);
        assert!(attr.form);
        assert_eq!(attr.middlewares.len(), 0);

        let item: ItemFn = parse_quote! {
            async fn revoke(ctx: HttpContext<AppState>, input: RevokeDto) -> Result<(), AnyError> {
                Ok(())
            }
        };
        let method = method_variant(&attr.method).unwrap();
        let expanded = server_arm(&attr, &item, &ActionState::of(&item).unwrap(), &method)
            .expect("expands")
            .to_string();
        assert!(expanded.contains("Route :: action_form"), "{expanded}");
        assert!(!expanded.contains("action_raw"), "{expanded}");
    }

    #[test]
    fn form_with_bytes_is_an_error() {
        let err = match syn::parse_str::<ActionArgs>(
            "POST, \"/x\", content_type = \"text/plain\", form, bytes",
        ) {
            Ok(_) => panic!("`form` with `bytes` must fail to parse"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("`form` cannot be combined with `raw_input` or `bytes`")
        );
    }

    #[test]
    fn accepts_form_with_bytes_binds_a_form_route_that_answers_with_a_file() {
        let attr: ActionArgs = syn::parse_quote!(
            POST,
            "/export",
            accepts_form,
            content_type = "application/json",
            bytes
        );
        assert!(attr.accepts_form && attr.bytes);
        assert_eq!(attr.middlewares.len(), 0);

        let item: ItemFn = parse_quote! {
            async fn export(ctx: HttpContext<AppState>, input: ExportDto) -> Result<Vec<u8>, AnyError> {
                Ok(Vec::new())
            }
        };
        let method = method_variant(&attr.method).unwrap();
        let expanded = server_arm(&attr, &item, &ActionState::of(&item).unwrap(), &method)
            .expect("expands")
            .to_string();
        assert!(
            expanded.contains("Route :: action_bytes_body_or_form"),
            "{expanded}"
        );
        assert!(expanded.contains("\"application/json\""), "{expanded}");
    }

    #[test]
    fn accepts_form_with_raw_input_is_an_error() {
        let err = match syn::parse_str::<ActionArgs>(
            "POST, \"/x\", content_type = \"text/plain\", accepts_form, raw_input",
        ) {
            Ok(_) => panic!("`accepts_form` with `raw_input` must fail to parse"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("`accepts_form` cannot be combined with `raw_input`"),
            "{err}"
        );
    }

    #[test]
    fn accepts_form_with_a_content_type_but_no_bytes_is_an_error() {
        let err = match syn::parse_str::<ActionArgs>(
            "POST, \"/x\", accepts_form, content_type = \"text/csv\"",
        ) {
            Ok(_) => panic!("a `content_type` that `accepts_form` would ignore must fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("add `bytes`"), "{err}");
    }

    #[test]
    fn raw_input_without_a_content_type_is_an_error() {
        let err = match syn::parse_str::<ActionArgs>("POST, \"/x\", raw_input") {
            Ok(_) => panic!("`raw_input` without a `content_type` must fail to parse"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("`raw_input` requires a `content_type`")
        );
    }

    #[test]
    fn raw_input_with_bytes_is_the_binary_io_shape() {
        let args = syn::parse_str::<ActionArgs>(
            "POST, \"/ohttp\", content_type = \"message/ohttp-res\", raw_input, bytes",
        )
        .expect("`raw_input` with `bytes` is the binary-in/binary-out shape");

        assert!(args.raw_input, "the request body is taken verbatim");
        assert!(args.bytes, "the response body is returned verbatim");
        assert_eq!(
            args.content_type.as_ref().map(syn::LitStr::value),
            Some("message/ohttp-res".to_string())
        );
    }

    #[test]
    fn no_single_object_is_a_marker_and_not_a_middleware() {
        let args = syn::parse_str::<ActionArgs>(
            "POST, \"/erase\", auth, scope(Subject::Erase), \
             no_single_object = \"the object would be the data subject\"",
        )
        .expect("the argument parses");

        assert_eq!(
            args.no_single_object
                .as_ref()
                .map(syn::LitStr::value)
                .as_deref(),
            Some("the object would be the data subject"),
            "the reason travels with the declaration"
        );
        assert_eq!(
            args.middlewares.len(),
            2,
            "the marker must not survive as a middleware: {:?}",
            args.middlewares
                .iter()
                .map(|expr| quote!(#expr).to_string())
                .collect::<Vec<_>>()
        );
    }

    /// Absence makes no claim, so the check fails closed.
    #[test]
    fn without_the_marker_a_route_claims_nothing() {
        let args = syn::parse_str::<ActionArgs>("POST, \"/revoke\", auth").expect("parses");
        assert!(args.no_single_object.is_none());
    }

    #[test]
    fn bytes_without_a_content_type_is_an_error() {
        let err = match syn::parse_str::<ActionArgs>("GET, \"/tile\", bytes") {
            Ok(_) => panic!("`bytes` without a `content_type` must fail to parse"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("`bytes` requires a `content_type`")
        );
    }

    #[test]
    fn middleware_list_emits_each_in_order() {
        let args = ActionArgs {
            method: Ident::new("POST", proc_macro2::Span::call_site()),
            path: parse_quote!("/x"),
            middlewares: vec![parse_quote!(auth), parse_quote!(rate_limit)],
            reactive: None,
            content_type: None,
            bytes: false,
            raw_input: false,
            form: false,
            accepts_form: false,
            no_single_object: None,
            cross_origin: None,
        };
        let item: ItemFn = parse_quote! {
            async fn x(ctx: HttpContext<AppState>, input: In) -> Result<Out, AnyError> { Ok(Out) }
        };
        let expanded = build(args, item).expect("expands").to_string();
        let auth = expanded.find("middleware (auth)").expect("auth listed");
        let rate_limit = expanded
            .find("middleware (rate_limit)")
            .expect("rate_limit listed");
        assert!(auth < rate_limit, "middlewares run in declared order");
    }
}

#[cfg(test)]
mod no_single_object_tests {
    use super::*;

    #[test]
    fn the_declaration_without_a_reason_is_refused() {
        assert!(
            syn::parse_str::<ActionArgs>("POST, \"/erase\", auth, no_single_object").is_err(),
            "a bare `no_single_object` states nothing a reviewer can weigh"
        );
    }

    #[test]
    fn a_placeholder_reason_is_refused() {
        assert!(
            syn::parse_str::<ActionArgs>("POST, \"/erase\", auth, no_single_object = \"tbd\"")
                .is_err(),
            "a placeholder is not a reason"
        );
    }
}

#[cfg(test)]
mod cross_origin_tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn the_set_is_carried_to_the_route_and_not_a_middleware() {
        let args = syn::parse_str::<ActionArgs>("POST, \"/chat\", budget, cross_origin = \"site\"")
            .expect("the argument parses");
        assert_eq!(
            args.cross_origin
                .as_ref()
                .map(syn::LitStr::value)
                .as_deref(),
            Some("site")
        );
        assert_eq!(args.middlewares.len(), 1, "only `budget` is a middleware");

        let item: ItemFn = parse_quote! {
            async fn open(ctx: HttpContext<AppState>, input: In) -> Result<Out, AnyError> { Ok(Out) }
        };
        let expanded = build(args, item).expect("expands").to_string();
        assert!(
            expanded.contains(". cross_origin (\"site\")"),
            "the route declares its set: {expanded}"
        );
    }

    #[test]
    fn a_route_without_a_named_set_is_refused() {
        assert!(syn::parse_str::<ActionArgs>("POST, \"/chat\", cross_origin").is_err());
        assert!(syn::parse_str::<ActionArgs>("POST, \"/chat\", cross_origin = \"\"").is_err());
    }
}
