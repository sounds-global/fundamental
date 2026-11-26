//! In a web crate, `mount_actions!` emits the server `register_actions` and a client `actions` module.
#![cfg(all(feature = "ssr", not(target_arch = "wasm32")))]

use fundamental_core::AnyError;
use fundamental_core::action::Outcome;
use fundamental_macros::{action, mount_actions};
use fundamental_web::Router;
use fundamental_web::prelude::*;

#[action(POST, "/api/greet")]
async fn greet(ctx: HttpContext<()>, name: String) -> Result<Outcome<String>, AnyError> {
    let _ = ctx;
    Ok(Outcome::changed(format!("hello, {name}")))
}

mod users {
    pub mod profile {
        mod get {
            use fundamental_core::AnyError;
            use fundamental_core::action::Outcome;
            use fundamental_macros::action;

            #[action(POST, "/api/users/get")]
            pub async fn get(
                ctx: HttpContext<()>,
                user_id: String,
            ) -> Result<Outcome<String>, AnyError> {
                let _ = ctx;
                Ok(Outcome::changed(user_id))
            }
        }

        pub use get::Get;

        mod search {
            use fundamental_core::AnyError;
            use fundamental_macros::action;

            #[action(POST, "/api/users/search", reactive(query))]
            pub async fn search(ctx: HttpContext<()>, query: String) -> Result<String, AnyError> {
                let _ = ctx;
                Ok(query)
            }
        }

        pub use search::Search;
    }
}

mod stats {
    mod read {
        use fundamental_core::AnyError;
        use fundamental_macros::action;

        #[action(GET, "/api/stats")]
        pub async fn read(_ctx: HttpContext<()>) -> Result<(), AnyError> {
            Ok(())
        }
    }

    pub use read::Read;
}

mount_actions! { Greet, users::profile::Get, users::profile::Search, stats::Read }

#[test]
fn mount_actions_emits_both_the_server_mount_and_the_client_stub() {
    let router: Router<()> = register_actions(Router::new()).expect("Greet mounts");
    assert_eq!(router.len(), 4);

    // Only a mounted action has a stub here.
    let _stub: fn(String) -> _ = actions::greet;
    let _nested_stub: fn(String) -> _ = actions::users::profile::get;
    let _zero_arg_stub: fn() -> _ = actions::stats::read;
    let _no_input_call = actions::stats::read();
    let _input_call = actions::users::profile::get(String::from("user-1"));

    let _greet_hook = actions::use_greet;
    let _stats_hook = actions::stats::use_stats_read;
    let _user_hook = actions::users::profile::use_users_profile_get;
    let _search_hook = actions::users::profile::use_users_profile_search;
}

#[component]
fn generated_hook_integrates_with_mutation() -> impl IntoView {
    let greet = actions::use_greet();
    let stats = actions::stats::use_stats_read();
    let user = actions::users::profile::use_users_profile_get();
    let search = actions::users::profile::use_users_profile_search().source(|| String::from("Ada"));
    let local_stats = actions::stats::use_stats_read().local();
    let local_search = actions::users::profile::use_users_profile_search()
        .source(|| String::from("Ada"))
        .local();
    let local_search_reordered = actions::users::profile::use_users_profile_search()
        .local()
        .source(|| String::from("Ada"));
    let local_greet = actions::use_greet().local();
    view! {
        <Mutation mutation=greet let:mutation>
            <button on:click=move |_| mutation.submit("Ada".to_owned())>
                "Greet"
            </button>
        </Mutation>
        <Mutation mutation=user let:mutation>
            <button on:click=move |_| mutation.submit("user-1".to_owned())>
                "Load user"
            </button>
        </Mutation>
        <Query resource=stats let:value>
            <p>{value}</p>
        </Query>
        <Query resource=search let:value>
            <p>{value}</p>
        </Query>
        <Query resource=local_stats let:value>
            <p>{format!("local stats: {value:?}")}</p>
        </Query>
        <Query resource=local_search let:value>
            <p>{value}</p>
        </Query>
        <Query resource=local_search_reordered let:value>
            <p>{value}</p>
        </Query>
        <Mutation mutation=local_greet let:mutation>
            <button on:click=move |_| mutation.submit("Ada".to_owned())>
                "Greet locally"
            </button>
        </Mutation>
    }
}

#[expect(
    dead_code,
    reason = "compiled to prove the generated hooks type-check, never called"
)]
fn mutation_hooks_accept_both_action_arities() {
    let no_input_send_action = fundamental_web::use_mutation(actions::stats::read);
    no_input_send_action.dispatch(());
    let input_send_action = fundamental_web::use_mutation(actions::greet);
    input_send_action.dispatch(String::from("Ada"));

    let no_input_local = fundamental_web::use_mutation_local(actions::stats::read);
    no_input_local.dispatch_local(());
    let input_local = fundamental_web::use_mutation_local(actions::users::profile::get);
    input_local.dispatch_local(String::from("user-1"));

    let _no_input_send = fundamental_web::use_mutation(|| async {});
    let _input_send = fundamental_web::use_mutation(|_: String| async {});
}
