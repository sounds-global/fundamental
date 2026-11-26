//! `#[derive(RpcSet)]` gives each request and reply its derived or overridden subject, one
//! descriptor per variant, and the subject lists.

use fundamental_nats::{Message, MessageDescriptor, Publish, RpcSet};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct EraseRequest {
    subject: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct EraseReply {
    count: u64,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct PingRequest {
    seq: u64,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct PingReply {
    seq: u64,
}

/// A request/reply set: one variant on the default subject, one overridden; replies append
/// `.reply`.
// Its variants are never constructed, so a private test enum trips `dead_code`.
#[allow(dead_code)]
#[derive(RpcSet)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
enum RpcLayer1 {
    /// Erase a subject.
    Erase(EraseRequest, EraseReply),
    #[subject = "acme.custom.ping"]
    Ping(PingRequest, PingReply),
}

#[test]
fn each_payload_gets_its_derived_or_overridden_subject() {
    assert_eq!(EraseRequest::SUBJECT, "rpc.layer1.erase");
    assert_eq!(EraseReply::SUBJECT, "rpc.layer1.erase.reply");
    assert_eq!(PingRequest::SUBJECT, "acme.custom.ping");
    assert_eq!(PingReply::SUBJECT, "acme.custom.ping.reply");
}

#[test]
fn the_set_lists_its_request_and_reply_subjects() {
    assert_eq!(
        RpcLayer1::REQUEST_SUBJECTS,
        ["rpc.layer1.erase", "acme.custom.ping"]
    );
    assert_eq!(
        RpcLayer1::REPLY_SUBJECTS,
        ["rpc.layer1.erase.reply", "acme.custom.ping.reply"]
    );
}

#[test]
fn payloads_are_publishable_messages_on_their_subject() {
    let request = EraseRequest {
        subject: "did:webvh:abc:holder".into(),
    };
    assert_eq!(Publish::subject(&request), "rpc.layer1.erase");
    assert_eq!(
        Message::to_bytes(&request).unwrap().as_ref(),
        br#"{"subject":"did:webvh:abc:holder"}"#
    );
}

#[test]
fn describe_set_yields_one_request_reply_descriptor_per_variant() {
    let descriptors: Vec<MessageDescriptor> = RpcLayer1::describe_set();
    let subjects: Vec<_> = descriptors.iter().map(|d| d.subject).collect();
    assert_eq!(subjects, ["rpc.layer1.erase", "acme.custom.ping"]);
    assert!(
        descriptors.iter().all(MessageDescriptor::is_request_reply),
        "every RPC descriptor carries a reply schema"
    );
}
