//! `LockedRelation` and `declared_namespace` resolved at boot, naming every problem.
// The schema enums are used only through their `Namespace`/`Relation` impls.
#![allow(missing_docs, dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use fundamental_authz::authorizer::LockedQuestions;
use fundamental_authz::schema::{
    LockedRelation, NamespaceExt, NamespaceName, RelationName, SchemaIr, declared_namespace,
};
use fundamental_authz::tuple::{CanonicalId, ObjectRef, SubjectRef};
use fundamental_core::validate::ValidationErrors;
use fundamental_macros::Namespace;

#[derive(Namespace)]
enum User {}

#[derive(Namespace)]
enum Customer {
    #[relation(direct(User))]
    #[locked]
    Editor,
    #[relation(direct(User))]
    Viewer,
}

fn schema() -> SchemaIr {
    SchemaIr::builder()
        .namespace::<User>()
        .namespace::<Customer>()
        .build()
        .expect("a valid schema")
}

fn code(problems: &ValidationErrors, field: &str) -> String {
    problems.errors_for_field(field)[0].code.to_string()
}

#[test]
fn a_declared_namespace_resolves_to_its_name() {
    let mut problems = ValidationErrors::new();
    let name = declared_namespace(&schema(), "customer", "subjects", &mut problems);
    assert_eq!(name, Some(Customer::name()));
    assert!(problems.is_empty());
}

#[test]
fn an_undeclared_or_malformed_namespace_is_named_by_its_field() {
    let schema = schema();
    let mut problems = ValidationErrors::new();
    assert_eq!(
        declared_namespace(&schema, "invoice", "subjects", &mut problems),
        None
    );
    assert_eq!(
        declared_namespace(&schema, "Not A Name", "actors", &mut problems),
        None
    );
    assert_eq!(code(&problems, "subjects"), "namespace_undeclared");
    assert_eq!(code(&problems, "actors"), "namespace_invalid");
}

#[test]
fn a_locked_relation_resolves_and_asks_its_own_question() {
    let mut problems = ValidationErrors::new();
    let space = Customer::name();
    let editor = LockedRelation::resolve(&schema(), &space, "editor", "edit", &mut problems)
        .expect("editor is declared #[locked]");
    assert!(problems.is_empty());
    assert_eq!(editor.namespace(), &space);
    assert_eq!(
        editor.relation(),
        &RelationName::new("editor").expect("a relation name")
    );

    let object = CanonicalId::new("c-1").expect("an id");
    let who = SubjectRef::leaf(User::name(), CanonicalId::new("u-1").expect("an id"));
    assert_eq!(
        editor.question(object.clone(), who.clone()),
        LockedQuestions::new(
            ObjectRef::new(space, object),
            editor.relation().clone(),
            who
        )
    );
}

#[test]
fn every_relation_that_cannot_be_locked_is_named_by_its_field() {
    let schema = schema();
    let space = Customer::name();
    let mut problems = ValidationErrors::new();
    for (raw, field) in [
        ("viewer", "view"),
        ("owner", "own"),
        ("Not A Relation", "edit"),
    ] {
        assert!(LockedRelation::resolve(&schema, &space, raw, field, &mut problems).is_none());
    }
    assert_eq!(code(&problems, "view"), "relation_not_locked");
    assert_eq!(code(&problems, "own"), "relation_undeclared");
    assert_eq!(code(&problems, "edit"), "relation_invalid");
}

#[test]
fn a_relation_in_an_undeclared_namespace_is_undeclared() {
    let mut problems = ValidationErrors::new();
    let space = NamespaceName::new("invoice").expect("a namespace name");
    assert!(LockedRelation::resolve(&schema(), &space, "editor", "edit", &mut problems).is_none());
    assert_eq!(code(&problems, "edit"), "relation_undeclared");
}
