//! Every spelling RFC 8785 does not write is refused, each with its own rule and offset.

use fundamental_canonical::{
    CanonicalError, Limits, Profile, from_canonical_slice, from_normal_slice, to_canonical_vec,
    validate_canonical,
};
use fundamental_core::Error as _;
use serde::{Deserialize, Serialize};

fn refusal(bytes: &[u8]) -> CanonicalError {
    match validate_canonical(bytes, &Limits::DOCUMENT, Profile::Verifier) {
        Ok(()) => panic!("accepted {:?}", String::from_utf8_lossy(bytes)),
        Err(error) => error,
    }
}

#[test]
fn strict_parse_refuses_every_listed_input() {
    let cases: &[(&[u8], &str)] = &[
        (b"{ }", "whitespace"),
        (b" 1", "whitespace"),
        (b"[1, 2]", "whitespace"),
        (b"{\"a\":1}\n", "whitespace"),
        (b"{\"a\" :1}", "whitespace"),
        (b"\xEF\xBB\xBF{}", "byte_order_mark"),
        (b"/*x*/1", "comment"),
        (b"[1,//x\n2]", "comment"),
        (b"1x", "trailing_bytes"),
        (b"{}{}", "trailing_bytes"),
        (br#"{"a":1,"a":2}"#, "duplicate_key"),
        (br#"{"b":1,"a":2}"#, "key_order"),
        // U+10000 sorts before U+E000 by UTF-16 code unit, after it by UTF-8 byte.
        ("{\"\u{E000}\":1,\"\u{10000}\":2}".as_bytes(), "key_order"),
        (b"1.0", "float"),
        (b"1e3", "float"),
        (b"1E3", "float"),
        (b"-0.5", "float"),
        (b"01", "non_canonical_integer"),
        (b"-0", "non_canonical_integer"),
        (b"+1", "non_canonical_integer"),
        (b"[-01]", "non_canonical_integer"),
        (b"9007199254740992", "integer_out_of_range"),
        (b"-9007199254740992", "integer_out_of_range"),
        (b"12345678901234567890", "integer_out_of_range"),
        (br#""\/""#, "forbidden_escape"),
        (br#""\u0041""#, "forbidden_escape"),
        (br#""\u000a""#, "forbidden_escape"),
        (br#""\u001F""#, "forbidden_escape"),
        (br#""\u00e9""#, "forbidden_escape"),
        (br#""\ud83d\ude00""#, "forbidden_escape"),
        (br#""\x""#, "forbidden_escape"),
        (br#""\ud83d""#, "lone_surrogate"),
        (br#""\ude00""#, "lone_surrogate"),
        (br#""\ud83da""#, "lone_surrogate"),
        (b"\"a\x01\"", "unescaped_control"),
        (b"\"a\nb\"", "unescaped_control"),
        (b"\"\xFF\"", "invalid_utf8"),
        (b"\"\xC0\xAF\"", "invalid_utf8"),
        (b"\"\xED\xA0\x80\"", "invalid_utf8"),
        (b"tru", "syntax"),
        (b"True", "syntax"),
        (b"NaN", "syntax"),
        (b"[1,]", "syntax"),
        (br#"{"a"}"#, "syntax"),
        (br#""open"#, "syntax"),
        (b"", "syntax"),
        (b"'a'", "syntax"),
    ];
    for (bytes, rule) in cases {
        let error = refusal(bytes);
        assert_eq!(
            error.rule(),
            *rule,
            "{:?} refused as {error:?}",
            String::from_utf8_lossy(bytes)
        );
        assert!(error.error_code().starts_with("CANON_"));
    }
}

#[test]
fn the_exact_integer_bounds_are_accepted() {
    for bytes in [&b"9007199254740991"[..], b"-9007199254740991", b"0", b"-1"] {
        assert!(validate_canonical(bytes, &Limits::DOCUMENT, Profile::Verifier).is_ok());
    }
}

#[test]
fn every_canonical_escape_is_accepted_and_decoded() {
    let text: String = from_canonical_slice(
        br#""\"\\\b\t\n\f\r\u0000\u001f""#.as_slice(),
        &Limits::DOCUMENT,
        Profile::Verifier,
    )
    .expect("every escape RFC 8785 writes");
    assert_eq!(text, "\"\\\u{08}\t\n\u{0C}\r\u{00}\u{1F}");
}

#[test]
fn delete_is_written_raw_so_its_escape_is_refused() {
    assert_eq!(refusal(br#""\u007f""#).rule(), "forbidden_escape");
}

#[test]
fn delete_and_non_ascii_are_written_raw() {
    let text: String = from_canonical_slice(
        "\"\u{7F}é😂\"".as_bytes(),
        &Limits::DOCUMENT,
        Profile::Verifier,
    )
    .expect("RFC 8785 writes these unescaped");
    assert_eq!(text, "\u{7F}é😂");
}

#[test]
fn every_bound_is_refused_with_its_own_rule() {
    let limits = Limits {
        max_bytes: 64,
        max_depth: 2,
        max_values: 3,
        max_string_bytes: 3,
        max_members: 2,
    };
    let refuse = |bytes: &[u8]| {
        validate_canonical(bytes, &limits, Profile::Verifier)
            .err()
            .map(|error| error.rule())
    };
    assert_eq!(refuse(&[b'1'; 65]), Some("too_large"));
    assert_eq!(refuse(b"[[[]]]"), Some("too_deep"));
    assert_eq!(refuse(b"[[1],2]"), Some("too_many_values"));
    assert_eq!(refuse(br#""abcd""#), Some("string_too_long"));
    assert_eq!(refuse(b"[1,1,1]"), Some("too_many_members"));
    assert_eq!(refuse(br#"{"a":1,"b":1,"c":1}"#), Some("too_many_members"));
    assert_eq!(refuse(b"[[1]]"), None);
}

#[test]
fn an_oversized_input_is_refused_before_it_is_read() {
    let error = refusal(&vec![b' '; Limits::DOCUMENT.max_bytes + 1]);
    assert!(matches!(error, CanonicalError::TooLarge { .. }));
    assert_eq!(error.http_status_code(), 413);
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Label {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[test]
fn a_shape_mismatch_is_refused_without_echoing_the_input() {
    let error = from_canonical_slice::<Label>(
        br#"{"name":"a","secret":1}"#,
        &Limits::DOCUMENT,
        Profile::Verifier,
    )
    .expect_err("an unknown member");
    assert_eq!(error.rule(), "shape");
    assert!(!error.to_string().contains("secret"));
}

#[test]
fn a_second_spelling_of_one_value_is_not_normal() {
    let written = to_canonical_vec(&Label {
        name: "a".into(),
        note: None,
    })
    .expect("a label");
    assert_eq!(written, br#"{"name":"a"}"#);
    assert!(from_normal_slice::<Label>(&written, &Limits::DOCUMENT, Profile::Verifier).is_ok());
    let error = from_normal_slice::<Label>(
        br#"{"name":"a","note":null}"#,
        &Limits::DOCUMENT,
        Profile::Verifier,
    )
    .expect_err("null is a second spelling of absent");
    assert!(matches!(error, CanonicalError::NotNormal));
}

#[test]
fn the_writer_refuses_what_has_no_canonical_encoding() {
    assert_eq!(
        to_canonical_vec(&1.5_f64).map_err(|error| error.rule()),
        Err("unrepresentable")
    );
    assert_eq!(
        to_canonical_vec(&9_007_199_254_740_992_u64).map_err(|error| error.rule()),
        Err("unrepresentable")
    );
    let mut keyed = std::collections::BTreeMap::new();
    keyed.insert(1_u8, 1_u8);
    assert_eq!(
        to_canonical_vec(&keyed).map_err(|error| error.rule()),
        Err("unrepresentable")
    );
}

#[cfg(feature = "registry")]
#[test]
fn the_registry_profile_refuses_what_the_verifier_accepts() {
    use fundamental_canonical::UnicodeData;

    let decomposed = "\"A\u{030A}\"".as_bytes();
    assert!(validate_canonical(decomposed, &Limits::DOCUMENT, Profile::Verifier).is_ok());
    let error =
        validate_canonical(decomposed, &Limits::DOCUMENT, Profile::Registry).expect_err("not NFC");
    assert_eq!(error.rule(), "not_nfc");

    // U+0378 is unassigned in every Unicode version to date.
    let unassigned = "\"\u{0378}\"".as_bytes();
    assert!(validate_canonical(unassigned, &Limits::DOCUMENT, Profile::Verifier).is_ok());
    let error = validate_canonical(unassigned, &Limits::DOCUMENT, Profile::Registry)
        .expect_err("unassigned");
    assert_eq!(error.rule(), "unassigned");
    assert_eq!(UnicodeData::REGISTRY.unicode, "17.0");
}

#[test]
fn keys_order_by_utf16_not_utf8() {
    // UTF-16 puts U+10000 (D800 DC00) below U+E000; UTF-8 puts it above.
    let astral_first = "{\"\u{10000}\":2,\"\u{E000}\":1}";
    let byte_order = "{\"\u{E000}\":1,\"\u{10000}\":2}";

    let mut members = std::collections::BTreeMap::new();
    members.insert("\u{E000}".to_owned(), 1_u8);
    members.insert("\u{10000}".to_owned(), 2_u8);
    let written = to_canonical_vec(&members).expect("two integer members have a canonical form");
    assert_eq!(
        written,
        astral_first.as_bytes(),
        "the writer ordered by UTF-8"
    );

    validate_canonical(
        astral_first.as_bytes(),
        &Limits::DOCUMENT,
        Profile::Verifier,
    )
    .expect("the UTF-16 order is canonical and must be read");
    assert_eq!(refusal(byte_order.as_bytes()).rule(), "key_order");
}
