//! End-to-end tests for `#[derive(Message)]` across struct shapes and subject modes.

use fundamental_nats::Message;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Message)]
#[subject = "events.user.created"]
struct UserCreated {
    user_id: String,
}

#[derive(Serialize, Deserialize, Message)]
#[subject = "ping"]
struct Ping;

#[derive(Serialize, Deserialize, Message)]
#[subject = "counts"]
struct CountEvent(u64);

#[derive(Serialize, Deserialize, Message)]
#[subject = "lifecycle"]
enum Lifecycle {
    Started,
    Stopped,
}

#[derive(Serialize, Deserialize, Message)]
#[subject_fn = "compute_subject"]
struct TenantEvent {
    tenant_id: String,
    kind: String,
}

impl TenantEvent {
    fn compute_subject(&self) -> String {
        format!("events.{}.{}", self.tenant_id, self.kind)
    }
}

#[derive(Serialize, Deserialize, Message)]
#[subject_fn = "subject_str"]
struct DynamicStr;

impl DynamicStr {
    fn subject_str(&self) -> &'static str {
        "computed.static"
    }
}

#[test]
fn static_subjects_on_every_shape() {
    assert_eq!(UserCreated::SUBJECT, "events.user.created");
    assert_eq!(Ping::SUBJECT, "ping");
    assert_eq!(CountEvent::SUBJECT, "counts");
    assert_eq!(Lifecycle::SUBJECT, "lifecycle");
}

#[test]
fn static_subject_method_borrows_const() {
    let m = UserCreated {
        user_id: "u1".into(),
    };
    assert_eq!(m.subject(), "events.user.created");
}

#[test]
fn dynamic_subject_uses_sentinel_const_and_computes_value() {
    assert_eq!(TenantEvent::SUBJECT, "__dynamic__");
    let m = TenantEvent {
        tenant_id: "acme".into(),
        kind: "signup".into(),
    };
    assert_eq!(m.subject(), "events.acme.signup");
}

#[test]
fn dynamic_subject_accepts_str_returning_method() {
    assert_eq!(DynamicStr::SUBJECT, "__dynamic__");
    assert_eq!(DynamicStr.subject(), "computed.static");
}

#[test]
fn derived_messages_roundtrip_bytes() {
    let m = UserCreated {
        user_id: "u9".into(),
    };
    let bytes = m.to_bytes().unwrap();
    let back = UserCreated::from_bytes(&bytes).unwrap();
    assert_eq!(back.user_id, "u9");
}
