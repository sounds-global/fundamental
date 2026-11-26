//! `#[derive(CanonicalJson)]` writes exactly what the canonical writer writes, and what the
//! strict reader accepts.

use fundamental_canonical::{
    CanonicalBytes, CanonicalJson, Limits, Profile, SafeInteger, from_canonical_slice,
    to_canonical_vec, validate_canonical,
};
use fundamental_canonical_macros::CanonicalJson;
use proptest::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(CanonicalJson)]
struct Derived {
    zeta: String,
    #[canonical(rename = "\u{10000}")]
    astral: u32,
    #[canonical(rename = "\u{E000}")]
    private_use: bool,
    alpha: Option<String>,
    list: Vec<i16>,
    count: SafeInteger,
}

/// The serde twin: the same members, encoded by the runtime writer.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Twin {
    zeta: String,
    #[serde(rename = "\u{10000}")]
    astral: u32,
    #[serde(rename = "\u{E000}")]
    private_use: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    alpha: Option<String>,
    list: Vec<i16>,
    count: i64,
}

fn text() -> impl Strategy<Value = String> {
    proptest::string::string_regex(
        "[\"\\\\\u{0}-\u{1F}\u{7F}a-z\u{E9}\u{2028}\u{E000}\u{10000}\u{1F602}]{0,12}",
    )
    .expect("a valid pattern")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    fn a_derived_document_is_its_jcs(
        zeta in text(),
        astral in any::<u32>(),
        private_use in any::<bool>(),
        alpha in proptest::option::of(text()),
        list in proptest::collection::vec(any::<i16>(), 0..5),
        count in -9_007_199_254_740_991_i64..=9_007_199_254_740_991,
    ) {
        let derived = Derived {
            zeta: zeta.clone(),
            astral,
            private_use,
            alpha: alpha.clone(),
            list: list.clone(),
            count: SafeInteger::try_from(count).expect("in range by construction"),
        };
        let twin = Twin { zeta, astral, private_use, alpha, list, count };
        let written = derived.to_canonical_json();
        prop_assert_eq!(&written, &to_canonical_vec(&twin).expect("the twin is representable"));
        let read: Twin = from_canonical_slice(&written, &Limits::DOCUMENT, Profile::Verifier)
            .expect("a derived document is canonical");
        prop_assert_eq!(read, twin);
    }
}

#[derive(CanonicalJson)]
struct Event<'a> {
    t: &'a str,
    by: &'a str,
    sub: &'a str,
    obj: Option<&'a str>,
    ts: &'a str,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct EventMirror {
    t: String,
    by: String,
    sub: String,
    obj: Option<String>,
    ts: String,
}

#[test]
fn a_quote_in_a_member_cannot_forge_another_member() {
    let forged = r#"urn:x","ts":"1970-01-01T00:00:00.000000Z"#;
    let event = Event {
        t: "grant",
        by: "https://a.example",
        sub: "https://b.example",
        obj: Some(forged),
        ts: "2026-09-21T10:00:00.000000Z",
    };
    let bytes = event.to_canonical_json();
    let read: EventMirror = from_canonical_slice(&bytes, &Limits::DOCUMENT, Profile::Verifier)
        .expect("the preimage is canonical and has exactly its five members");
    assert_eq!(read.obj.as_deref(), Some(forged));
    assert_eq!(read.ts, "2026-09-21T10:00:00.000000Z");
}

#[test]
fn an_absent_optional_member_is_omitted() {
    let event = Event {
        t: "x",
        by: "b",
        sub: "s",
        obj: None,
        ts: "t",
    };
    assert_eq!(
        event.to_canonical_json(),
        br#"{"by":"b","sub":"s","t":"x","ts":"t"}"#
    );
}

#[derive(CanonicalJson)]
struct Generic<T> {
    value: T,
}

#[test]
fn a_generic_member_is_written_through_its_own_encoding() {
    assert_eq!(
        Generic {
            value: vec![1_u8, 2]
        }
        .to_canonical_json(),
        br#"{"value":[1,2]}"#
    );
    let nested = Generic {
        value: CanonicalBytes::of(&["a", "b"]).expect("strings"),
    };
    let bytes = nested.to_canonical_json();
    assert_eq!(bytes, br#"{"value":["a","b"]}"#);
    validate_canonical(&bytes, &Limits::DOCUMENT, Profile::Verifier).expect("canonical");
}

#[derive(CanonicalJson)]
struct Empty {}

#[test]
fn a_struct_with_no_members_is_the_empty_object() {
    assert_eq!(Empty {}.to_canonical_json(), b"{}");
}
