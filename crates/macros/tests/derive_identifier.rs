//! Trait-level tests for `#[derive(Identifier)]`, without backend bindings.

use fundamental::{GeneratedId, Handle, HasHandle, Identifier};

// No backend features here; the bindings are exercised in the backend regression crate.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Identifier)]
#[identifier(no_sql, no_cql)]
struct UserId(u64);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Identifier)]
#[identifier(generate = "uuid_v4", no_sql, no_cql)]
struct SessionId(uuid::Uuid);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Identifier)]
#[identifier(generate = "uuid_v7", no_display, no_sql, no_cql)]
struct EventId(uuid::Uuid);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Identifier)]
#[identifier(generate = "uuid_v7", handle = "build_request", no_sql, no_cql)]
struct BuildRequestId(uuid::Uuid);

#[test]
fn a_declared_handle_spells_and_reads_the_id() {
    assert_eq!(BuildRequestId::HANDLE_PREFIX.as_str(), "build_request");
    let id = BuildRequestId::generate();
    let text = Handle::of(&id).to_string();
    assert!(text.starts_with("build_request_"), "{text}");
    assert_eq!(
        Handle::<BuildRequestId>::parse(&text).map(|h| h.id()),
        Ok(id.clone())
    );
    // The id's own text form is untouched: the offline codec still reads it.
    assert_eq!(id.to_string(), id.as_inner().to_string());
}

#[test]
fn identifier_trait_round_trip() {
    let id = UserId::from_inner(42);
    assert_eq!(*id.as_inner(), 42);
    assert_eq!(id.clone().into_inner(), 42);
    let from: UserId = 42u64.into();
    assert_eq!(from, id);
}

#[test]
fn display_delegates_to_inner() {
    let id = UserId(7);
    assert_eq!(id.to_string(), "7");
}

#[test]
fn generated_id_mints_unique_values() {
    let a = SessionId::generate();
    let b = SessionId::generate();
    assert_ne!(a, b);

    // `no_display` leaves no `Display` impl; Debug still works.
    let e1 = EventId::generate();
    let e2 = EventId::generate();
    assert_ne!(e1, e2);
}
