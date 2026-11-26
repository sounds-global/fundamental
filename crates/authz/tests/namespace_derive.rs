//! `#[derive(Namespace)]` lowers to exactly the runtime IR and assembles a valid schema.
// The schema enums are used only through their `Namespace`/`Relation` impls.
#![allow(missing_docs, dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use fundamental_authz::schema::{Namespace, Relation, RewriteIr, SchemaIr};
use fundamental_authz::schema::{NamespaceName, RelationName};
use fundamental_macros::Namespace;

fn nn(s: &str) -> NamespaceName {
    NamespaceName::new(s).expect("valid namespace")
}
fn rn(s: &str) -> RelationName {
    RelationName::new(s).expect("valid relation")
}

#[derive(Namespace)]
enum User {}

#[derive(Namespace)]
enum Group {
    #[relation(direct(User, Group::Member))]
    Member,
}

#[derive(Namespace)]
enum Folder {
    #[relation(direct(User))]
    Owner,
    #[relation(union(Owner, direct(User, Group::Member)))]
    Viewer,
}

// A document whose viewers are its editors plus its parent folder's, minus the banned.
#[derive(Namespace)]
#[namespace("document")]
enum Document {
    #[relation(direct(User, Group::Member))]
    Owner,
    #[relation(union(Owner, direct(User)))]
    Editor,
    #[relation(direct(Folder))]
    Parent,
    #[relation(exclusion(union(Editor, tuple_to_userset(Parent, Folder::Viewer)), Banned))]
    Viewer,
    #[relation(direct(User))]
    Banned,
}

#[test]
fn namespace_name_defaults_to_snake_case_and_overrides() {
    assert_eq!(Group::NAME, "group");
    assert_eq!(Folder::NAME, "folder");
    assert_eq!(Document::NAME, "document");
}

#[test]
fn relation_names_are_snake_cased_variants() {
    assert_eq!(Document::Owner.relation_name(), "owner");
    assert_eq!(Document::Viewer.relation_name(), "viewer");
    assert_eq!(Group::Member.relation_name(), "member");
}

#[test]
fn direct_relation_lowers_to_this_with_type_restrictions() {
    let ir = Group::ir();
    let member = ir.relation(&rn("member")).expect("member relation");
    assert_eq!(member.rewrite, RewriteIr::This);
    assert_eq!(member.directly_related_types.len(), 2);
    assert!(
        member
            .directly_related_types
            .iter()
            .any(|t| t.namespace.as_str() == "user" && t.relation.is_none())
    );
    assert!(
        member
            .directly_related_types
            .iter()
            .any(|t| t.namespace.as_str() == "group"
                && t.relation.as_ref().map(|r| r.as_str()) == Some("member"))
    );
}

#[test]
fn union_computed_and_direct_lower_correctly() {
    let ir = Folder::ir();
    let viewer = ir.relation(&rn("viewer")).expect("viewer relation");
    match &viewer.rewrite {
        RewriteIr::Union(children) => {
            assert_eq!(children.len(), 2);
            assert_eq!(
                children[0],
                RewriteIr::ComputedUserset {
                    relation: rn("owner")
                }
            );
            assert_eq!(children[1], RewriteIr::This);
        }
        other => panic!("expected union, got {other:?}"),
    }
    assert_eq!(viewer.directly_related_types.len(), 2);
}

#[test]
fn tuple_to_userset_and_exclusion_lower_correctly() {
    let ir = Document::ir();
    let viewer = ir.relation(&rn("viewer")).expect("viewer relation");
    match &viewer.rewrite {
        RewriteIr::Exclusion { include, exclude } => {
            assert_eq!(
                **exclude,
                RewriteIr::ComputedUserset {
                    relation: rn("banned")
                }
            );
            match include.as_ref() {
                RewriteIr::Union(children) => {
                    assert_eq!(
                        children[0],
                        RewriteIr::ComputedUserset {
                            relation: rn("editor")
                        }
                    );
                    assert_eq!(
                        children[1],
                        RewriteIr::TupleToUserset {
                            tupleset: rn("parent"),
                            computed: rn("viewer"),
                        }
                    );
                }
                other => panic!("expected union in include, got {other:?}"),
            }
        }
        other => panic!("expected exclusion, got {other:?}"),
    }
}

// `Offer` is a scope with no per-object meaning, so it declares no relation.
#[derive(Namespace)]
enum Credential {
    #[relation(direct(Folder))]
    Issuer,
    #[relation(skip)]
    Offer,
    #[relation(union(direct(User), tuple_to_userset(Issuer, Folder::Viewer)))]
    Revoke,
}

/// A skipped variant defines no relation, so a guard on it fails visibly.
#[test]
fn a_skipped_variant_declares_no_relation() {
    let ir = Credential::ir();
    let names: Vec<&str> = ir.relations.keys().map(RelationName::as_str).collect();
    assert_eq!(names, vec!["issuer", "revoke"], "offer must be absent");
    assert!(ir.relation(&rn("offer")).is_none());
}

/// `Relation::relation_name` covers every variant, even those naming no relation.
#[test]
fn a_skipped_variant_keeps_its_relation_name() {
    assert_eq!(Credential::Offer.relation_name(), "offer");
}

#[test]
fn typed_namespaces_assemble_into_a_valid_schema() {
    let schema = SchemaIr::builder()
        .namespace::<User>()
        .namespace::<Group>()
        .namespace::<Folder>()
        .namespace::<Document>()
        .build()
        .expect("typed schema compiles");
    assert_eq!(schema.len(), 4);
    assert!(schema.relation(&nn("document"), &rn("viewer")).is_some());
}

#[derive(Namespace)]
enum Squad {
    #[relation(direct(User))]
    Member,
    #[relation(direct(User))]
    Banned,
    #[relation(exclusion(Member, Banned))]
    Active,
}

#[derive(Namespace)]
enum Ledger {
    #[relation(direct(User))]
    #[locked]
    Keeper,
    #[relation(direct(Squad::Active))]
    #[locked]
    Writer,
}

#[derive(Namespace)]
enum Journal {
    #[relation(direct(User))]
    #[locked]
    Keeper,
    #[relation(union(Keeper, direct(User)))]
    Reader,
}

#[test]
fn a_locked_variant_lowers_to_a_locked_relation() {
    let ir = Journal::ir();
    assert!(ir.relations[&rn("keeper")].locked);
    assert!(!ir.relations[&rn("reader")].locked);

    let schema = SchemaIr::builder()
        .namespace::<User>()
        .namespace::<Journal>()
        .build()
        .expect("a locked monotone relation compiles");
    assert!(
        schema
            .relation(&nn("journal"), &rn("keeper"))
            .is_some_and(|r| r.locked)
    );
}

#[test]
fn a_locked_relation_reaching_an_exclusion_in_another_namespace_is_refused_at_load() {
    use fundamental_authz::AuthzError;
    use fundamental_core::Error as _;

    let error = SchemaIr::builder()
        .namespace::<User>()
        .namespace::<Squad>()
        .namespace::<Ledger>()
        .build()
        .expect_err("ledger#writer reaches squad#active, an exclusion");
    assert_eq!(error.error_code(), "AUTHZ_021");
    match error {
        AuthzError::LockedRelationReachesExclusion {
            namespace,
            relation,
            exclusion,
            path,
        } => {
            assert_eq!(
                (namespace.as_str(), relation.as_str()),
                ("ledger", "writer")
            );
            assert_eq!(exclusion, "squad#active");
            assert_eq!(path, "ledger#writer -> squad#active");
        }
        other => panic!("unexpected: {other:?}"),
    }
}
