//! The reader and the writer agree on every value, and the reader accepts exactly the
//! writer's output.

use fundamental_canonical::{Limits, Profile, from_canonical_slice, to_canonical_vec};
use proptest::prelude::*;
use serde_json::{Map, Value};

const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;

fn value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).prop_map(Value::from),
        any::<String>().prop_map(Value::String),
        // Keys and strings from the planes where UTF-8 and UTF-16 order disagree.
        "[\u{0}-\u{1F}\u{7F}\u{E000}-\u{E0FF}\u{10000}-\u{100FF}a-c]{0,6}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            proptest::collection::btree_map(
                "[\u{0}-\u{1F}\u{E000}\u{10000}a-c\"\\\\]{0,4}",
                inner,
                0..6
            )
            .prop_map(|members| Value::Object(members.into_iter().collect::<Map<String, Value>>())),
        ]
    })
}

fn strict(bytes: &[u8]) -> Result<Value, fundamental_canonical::CanonicalError> {
    from_canonical_slice(bytes, &Limits::DOCUMENT, Profile::Verifier)
}

fn mutated(bytes: &[u8], at: usize, byte: u8, kind: u8) -> Vec<u8> {
    let mut out = bytes.to_vec();
    if out.is_empty() {
        out.push(byte);
        return out;
    }
    let at = at % out.len();
    match kind % 3 {
        0 => out[at] = byte,
        1 => out.insert(at, byte),
        _ => {
            out.remove(at);
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4_000))]

    #[test]
    fn every_written_value_is_read_back_and_rewritten_identically(value in value()) {
        let bytes = to_canonical_vec(&value).expect("every generated value is representable");
        let read = strict(&bytes).expect("the writer's output is canonical");
        prop_assert_eq!(&read, &value);
        prop_assert_eq!(to_canonical_vec(&read).expect("representable"), bytes);
    }

    #[test]
    fn strict_parse_succeeds_iff_jcs_round_trips(
        value in value(),
        at in any::<usize>(),
        byte in any::<u8>(),
        kind in any::<u8>(),
    ) {
        let bytes = mutated(&to_canonical_vec(&value).expect("representable"), at, byte, kind);
        let strict_result = strict(&bytes);
        // Accepted ⇒ the bytes are the canonical encoding of what was read.
        if let Ok(read) = &strict_result {
            prop_assert_eq!(to_canonical_vec(read).expect("representable"), bytes.clone());
        }
        // Canonical by an independent, lenient parse ⇒ accepted.
        let canonical_elsewhere = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|lenient| to_canonical_vec(&lenient).ok())
            .is_some_and(|rewritten| rewritten == bytes);
        if canonical_elsewhere {
            prop_assert!(strict_result.is_ok(), "refused canonical bytes: {:?}", String::from_utf8_lossy(&bytes));
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _ = strict(&bytes);
    }
}
