//! A relation is a type: `Authorized<N, R>` yields an object only for a grant at exactly `R`.

#![cfg(all(feature = "server", not(target_arch = "wasm32")))]

use fundamental_authz::middleware::Authorized;
use fundamental_authz::schema::{Namespace, Relation, RelationOf};
use fundamental_macros::Namespace;
use fundamental_server::AuthorizedObjects;

#[derive(Namespace)]
enum User {}

#[derive(Namespace)]
enum Document {
    #[relation(direct(User))]
    Owner,
    #[relation(union(Owner, direct(User)))]
    Viewer,
    #[relation(skip)]
    Audit,
}

fn relation_of<R: RelationOf<Document>>() -> &'static str {
    R::relation_name()
}

#[test]
fn each_relation_is_named_by_its_own_type() {
    assert_eq!(relation_of::<document_relation::Owner>(), "owner");
    assert_eq!(relation_of::<document_relation::Viewer>(), "viewer");
    assert!(matches!(
        <document_relation::Owner as RelationOf<Document>>::RELATION,
        Document::Owner
    ));
    assert_eq!(Document::NAME, "document");
    // A skipped variant keeps its name but defines no relation, so it has no marker type.
    assert_eq!(Document::Audit.relation_name(), "audit");
}

#[test]
fn authorized_at_a_relation_reads_only_that_relation() {
    let objects = AuthorizedObjects::default();
    objects.record("document", "viewer", "readme");

    let viewer = Authorized::<Document, document_relation::Viewer>::granted_in(&objects)
        .expect("a viewer check allowed");
    assert_eq!(viewer.id(), "readme");
    assert!(
        Authorized::<Document, document_relation::Owner>::granted_in(&objects).is_none(),
        "a viewer grant is not an owner grant, though every owner is a viewer"
    );

    objects.record("document", "owner", "readme");
    assert!(Authorized::<Document, document_relation::Owner>::granted_in(&objects).is_some());

    objects.record("document", "viewer", "other");
    assert!(
        Authorized::<Document, document_relation::Viewer>::granted_in(&objects).is_none(),
        "two documents leave no single answer"
    );
}

#[test]
fn a_namespace_declared_in_a_function_has_its_relation_types() {
    #[derive(Namespace)]
    enum Viewer {}

    #[derive(Namespace)]
    enum Folder {
        #[relation(direct(Viewer))]
        Reader,
    }

    assert_eq!(
        <folder_relation::Reader as RelationOf<Folder>>::relation_name(),
        "reader"
    );
}
