//! The `#[action]` browser arm compiles to wasm and resolves the web call transport.
#![cfg(target_arch = "wasm32")]

use fundamental_core::AnyError;
use fundamental_core::action::Outcome;
use fundamental_macros::{action, mount_actions};
use fundamental_web::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen_test::wasm_bindgen_test;

// A reactive action clones its input into the Leptos `Action`.
#[derive(Serialize, Clone)]
struct NewUser {
    name: String,
}

// Written by the `Deserialize` derive but never read: this fixture only type-checks.
#[derive(Clone, Deserialize)]
#[expect(dead_code, reason = "the stub reads its type; nothing decodes one")]
struct User {
    id: u64,
}

// The handler is emitted only on the non-wasm arm, so `HttpContext` is never resolved here.
#[action(POST, "/users")]
async fn create_user(
    ctx: fundamental_web::HttpContext<()>,
    input: NewUser,
) -> Result<User, AnyError> {
    let _ = ctx;
    Ok(User {
        id: input.name.len() as u64,
    })
}

#[action(GET, "/health")]
async fn health(ctx: fundamental_web::HttpContext<()>) -> Result<bool, AnyError> {
    let _ = ctx;
    Ok(true)
}

#[action(POST, "/register")]
async fn register_user(
    ctx: fundamental_web::HttpContext<()>,
    input: NewUser,
) -> Result<Outcome<User>, AnyError> {
    let _ = ctx;
    Ok(Outcome::changed(User {
        id: input.name.len() as u64,
    }))
}

#[action(POST, "/refresh")]
async fn refresh(ctx: fundamental_web::HttpContext<()>) -> Result<Outcome<()>, AnyError> {
    let _ = ctx;
    Ok(Outcome::nothing())
}

mount_actions! { CreateUser, Health, RegisterUser, Refresh }

#[wasm_bindgen_test]
fn the_browser_stubs_type_check() {
    // Referenced as values so their bodies type-check; invoking them would need a server.
    let _create = create_user;
    let _health = health;
    let _register = register_user;
    let _hook = actions::use_register_user;
}

#[expect(dead_code, reason = "a compile-time check of the hook; never called")]
fn mutation_hooks_accept_zero_argument_actions() {
    let send_values = fundamental_web::use_mutation(health);
    send_values.dispatch(());

    let local_values = fundamental_web::use_mutation_local(health);
    local_values.dispatch_local(());
}

#[component]
fn query_and_mutation_components_compile() -> impl IntoView {
    let stats = fundamental_web::use_query!(health);
    let prefetched = fundamental_web::use_query_prefetch!(health);
    let refresh_stats = fundamental_web::use_mutation(refresh);
    let local_refresh_stats = fundamental_web::use_mutation_local(refresh);
    let query_error = AsyncErrorView::new(|error, stale, _retry| {
        view! { <div>{stale}<p>{error.to_string()}</p></div> }
    });
    let mutation_success = |outcome: Outcome<()>| view! { <p>{outcome.said}</p> };
    let mutation_error =
        AsyncErrorView::new(|error, _stale, _retry| view! { <p>{error.to_string()}</p> });

    view! {
        <AsyncDefaults
            query_pending=|| view! { <p>"Loading…"</p> }
            query_refreshing=|| view! { <small>"Refreshing…"</small> }
            query_error=query_error
            mutation_pending=|| view! { <p>"Saving…"</p> }
            mutation_success=mutation_success
            mutation_error=mutation_error
        >
            <Query resource=stats let:value>
                <p>{value}</p>
            </Query>
            <Query resource=prefetched let:value>
                <p>{value}</p>
            </Query>
            <Mutation mutation=refresh_stats let:mutation>
                <button on:click=move |_| mutation.submit(())>"Refresh"</button>
            </Mutation>
            <Mutation mutation=local_refresh_stats let:mutation>
                <button on:click=move |_| mutation.submit(())>"Refresh locally"</button>
            </Mutation>
        </AsyncDefaults>
    }
}

#[component]
fn generated_action_hooks_compile_on_wasm() -> impl IntoView {
    let register = actions::use_register_user();
    let health = actions::use_health();
    view! {
        <Mutation mutation=register let:mutation>
            <button on:click=move |_| mutation.submit(NewUser { name: "Ada".into() })>
                "Register"
            </button>
        </Mutation>
        <Query resource=health let:value>
            <p>{if value { "Healthy" } else { "Unavailable" }}</p>
        </Query>
    }
}
