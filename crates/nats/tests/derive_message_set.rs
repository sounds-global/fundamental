//! `#[derive(MessageSet)]` routes each variant to its subject, round-trips the inner payload and
//! rejects an unclaimed subject.

use fundamental_nats::{MessageSet, NatsError, Publish};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct UserCreated {
    user_id: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct OrderPlaced {
    id: u64,
}

/// A payload-carrying set: one variant on the enum-namespaced default subject, one overridden.
#[derive(MessageSet, PartialEq, Debug)]
enum Events {
    UserCreated(UserCreated),
    #[subject = "shop.orders.placed"]
    OrderPlaced(OrderPlaced),
}

#[test]
fn subjects_default_to_the_dotted_namespace_or_the_override() {
    assert_eq!(
        Events::SUBJECTS,
        ["events.user.created", "shop.orders.placed"]
    );
    assert_eq!(
        Events::UserCreated(UserCreated {
            user_id: "u1".into()
        })
        .subject(),
        "events.user.created"
    );
    assert_eq!(
        Events::OrderPlaced(OrderPlaced { id: 7 }).subject(),
        "shop.orders.placed"
    );
}

#[test]
fn to_bytes_serializes_the_inner_payload_not_the_enum() {
    let event = Events::UserCreated(UserCreated {
        user_id: "u1".into(),
    });
    let bytes = event.to_bytes().unwrap();
    assert_eq!(bytes.as_ref(), br#"{"user_id":"u1"}"#);
}

#[test]
fn from_parts_routes_each_subject_back_to_its_variant() {
    let user = Events::from_parts("events.user.created", br#"{"user_id":"u1"}"#).unwrap();
    assert_eq!(
        user,
        Events::UserCreated(UserCreated {
            user_id: "u1".into()
        })
    );

    let order = Events::from_parts("shop.orders.placed", br#"{"id":7}"#).unwrap();
    assert_eq!(order, Events::OrderPlaced(OrderPlaced { id: 7 }));
}

#[test]
fn round_trip_through_subject_and_bytes() {
    let event = Events::OrderPlaced(OrderPlaced { id: 42 });
    let subject = event.subject();
    let bytes = event.to_bytes().unwrap();
    let decoded = Events::from_parts(&subject, &bytes).unwrap();
    assert_eq!(decoded, event);
}

#[test]
fn from_parts_rejects_an_unclaimed_subject() {
    let err = Events::from_parts("events.stray", b"{}").unwrap_err();
    assert!(
        matches!(&err, NatsError::UnknownSubject { subject } if subject == "events.stray"),
        "expected UnknownSubject, got {err:?}"
    );
}

#[test]
fn from_parts_rejects_an_undecodable_payload() {
    let err = Events::from_parts("events.user.created", b"not json").unwrap_err();
    assert!(
        matches!(err, NatsError::Serialization(_)),
        "expected Serialization, got {err:?}"
    );
}

#[test]
fn describe_set_yields_one_descriptor_per_variant() {
    let descriptors = Events::describe_set();
    let subjects: Vec<_> = descriptors.iter().map(|d| d.subject).collect();
    assert_eq!(subjects, ["events.user.created", "shop.orders.placed"]);
}
