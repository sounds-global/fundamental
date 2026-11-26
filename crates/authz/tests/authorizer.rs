//! The [`Authorizer`] surface and the fluent [`AuthorizerExt`] over it.
#![cfg(all(feature = "offline", not(target_arch = "wasm32")))]
#![allow(missing_docs, dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::sync::Arc;

use fundamental_authz::authorizer::{AuthorizerExt, NativeEngine};
use fundamental_authz::authz_schema;
use fundamental_authz::backend::OfflineTupleStore;
use fundamental_authz::schema::{NamespaceExt, RelationExt, SchemaIr};
use fundamental_authz::zookie::BackendTag;
use fundamental_macros::Namespace;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};

#[derive(Namespace)]
enum User {}

#[derive(Namespace)]
enum Group {
    #[relation(direct(User, Group::Member))]
    Member,
}

#[derive(Namespace)]
enum Document {
    #[relation(direct(User, Group::Member))]
    Owner,
    #[relation(union(Owner, direct(User, Group::Member)))]
    Viewer,
}

fn schema() -> Arc<SchemaIr> {
    Arc::new(authz_schema![User, Group, Document].expect("valid schema"))
}

async fn native() -> NativeEngine<OfflineTupleStore> {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open");
    let store = OfflineTupleStore::new(db.into_shared());
    store.ensure_schema().await.expect("schema");
    NativeEngine::new(store, schema())
}

#[tokio::test]
async fn grant_then_check_reflects_it() {
    let authz = native().await;

    assert!(
        !authz
            .can(User::subject("alice"), Document::Viewer, "readme")
            .await
            .expect("check")
    );

    let zookie = authz
        .grant(User::subject("alice"), Document::Owner, "readme")
        .await
        .expect("grant");

    // A check at least as fresh as that write sees alice as a viewer (owner ⇒ viewer).
    let after = authz
        .can(User::subject("alice"), Document::Viewer, "readme")
        .at_least_as_fresh(zookie)
        .response()
        .await
        .expect("check");
    assert!(after.allowed);
    assert_eq!(after.evaluated_at.backend(), BackendTag::Offline);
}

#[tokio::test]
async fn revoke_removes_access() {
    let authz = native().await;
    authz
        .grant(User::subject("bob"), Document::Owner, "spec")
        .await
        .expect("grant");
    assert!(
        authz
            .can(User::subject("bob"), Document::Viewer, "spec")
            .await
            .unwrap()
    );

    authz
        .revoke(User::subject("bob"), Document::Owner, "spec")
        .await
        .expect("revoke");
    assert!(
        !authz
            .can(User::subject("bob"), Document::Viewer, "spec")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_group_userset_subject_grants_indirectly() {
    let authz = native().await;
    authz
        .grant(User::subject("alice"), Group::Member, "eng")
        .await
        .expect("grant membership");
    authz
        .grant(Group::Member.userset("eng"), Document::Owner, "readme")
        .await
        .expect("grant to the userset");

    assert!(
        authz
            .can(User::subject("alice"), Document::Viewer, "readme")
            .await
            .unwrap()
    );
    assert!(
        !authz
            .can(User::subject("mallory"), Document::Viewer, "readme")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn list_returns_the_objects_a_subject_holds() {
    let authz = native().await;
    authz
        .grant(User::subject("carol"), Document::Owner, "a")
        .await
        .unwrap();
    authz
        .grant(User::subject("carol"), Document::Owner, "b")
        .await
        .unwrap();

    let mut owned = authz
        .list(User::subject("carol"), Document::Owner)
        .await
        .expect("list");
    owned.sort_by(|l, r| l.id.as_str().cmp(r.id.as_str()));
    let ids: Vec<&str> = owned.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
}

#[tokio::test]
async fn ergonomic_api_works_on_the_object_safe_handle() {
    use fundamental_authz::authorizer::{DynAuthorizer, HasAuthorizer};

    // `check_typed` is not on `DynAuthorizer`, but the fluent `AuthorizerExt` is.
    let authz: Arc<dyn DynAuthorizer> = Arc::new(native().await);

    struct AppState {
        authz: Arc<dyn DynAuthorizer>,
    }
    impl HasAuthorizer for AppState {
        fn authorizer(&self) -> &dyn DynAuthorizer {
            self.authz.as_ref()
        }
    }

    let state = AppState { authz };
    state
        .authorizer()
        .grant(User::subject("carol"), Document::Owner, "plan")
        .await
        .expect("grant via dyn handle");

    assert!(
        state
            .authorizer()
            .can(User::subject("carol"), Document::Viewer, "plan")
            .await
            .expect("check via dyn handle")
    );
    assert!(
        !state
            .authorizer()
            .can(User::subject("dave"), Document::Viewer, "plan")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn an_invalid_reference_surfaces_as_an_error_not_a_deny() {
    use fundamental_core::Error;
    let authz = native().await;
    let err = authz
        .can(User::subject("alice"), Document::Viewer, "bad id")
        .await
        .expect_err("invalid object id");
    assert_eq!(err.error_code(), "AUTHZ_006");
}

#[tokio::test]
async fn a_wildcard_grant_reaches_everyone() {
    let authz = native().await;
    authz
        .grant(User::wildcard(), Document::Viewer, "handbook")
        .await
        .expect("grant wildcard");
    assert!(
        authz
            .can(User::subject("anyone"), Document::Viewer, "handbook")
            .await
            .unwrap()
    );
}
