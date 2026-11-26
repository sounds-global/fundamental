//! The TypeID v0.3 specification's own test vectors, read through [`TypeIdText`].
//!
//! Vendored from `spec/valid.yml` and `spec/invalid.yml` of
//! <https://github.com/jetify-com/typeid> at revision
//! `be8ff0daf5dc1f6d40c62a03cfc89945263a69af`. A vector commented out upstream is left out here.

use fundamental_core::model::{Opaque128, TypeIdText};

/// `(name, typeid, prefix, uuid)`.
const VALID: &[(&str, &str, &str, &str)] = &[
    (
        "nil",
        "00000000000000000000000000",
        "",
        "00000000-0000-0000-0000-000000000000",
    ),
    (
        "one",
        "00000000000000000000000001",
        "",
        "00000000-0000-0000-0000-000000000001",
    ),
    (
        "ten",
        "0000000000000000000000000a",
        "",
        "00000000-0000-0000-0000-00000000000a",
    ),
    (
        "sixteen",
        "0000000000000000000000000g",
        "",
        "00000000-0000-0000-0000-000000000010",
    ),
    (
        "thirty-two",
        "00000000000000000000000010",
        "",
        "00000000-0000-0000-0000-000000000020",
    ),
    (
        "max-valid",
        "7zzzzzzzzzzzzzzzzzzzzzzzzz",
        "",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
    ),
    (
        "valid-alphabet",
        "prefix_0123456789abcdefghjkmnpqrs",
        "prefix",
        "0110c853-1d09-52d8-d73e-1194e95b5f19",
    ),
    (
        "valid-uuidv7",
        "prefix_01h455vb4pex5vsknk084sn02q",
        "prefix",
        "01890a5d-ac96-774b-bcce-b302099a8057",
    ),
    (
        "prefix-underscore",
        "pre_fix_00000000000000000000000000",
        "pre_fix",
        "00000000-0000-0000-0000-000000000000",
    ),
];

/// `(name, typeid)`.
const INVALID: &[(&str, &str)] = &[
    ("prefix-uppercase", "PREFIX_00000000000000000000000000"),
    ("prefix-numeric", "12345_00000000000000000000000000"),
    ("prefix-period", "pre.fix_00000000000000000000000000"),
    ("prefix-non-ascii", "préfix_00000000000000000000000000"),
    ("prefix-spaces", "  prefix_00000000000000000000000000"),
    (
        "prefix-64-chars",
        "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijkl_00000000000000000000000000",
    ),
    ("separator-empty-prefix", "_00000000000000000000000000"),
    ("separator-empty", "_"),
    ("suffix-short", "prefix_1234567890123456789012345"),
    ("suffix-long", "prefix_123456789012345678901234567"),
    ("suffix-spaces", "prefix_1234567890123456789012345 "),
    ("suffix-uppercase", "prefix_0123456789ABCDEFGHJKMNPQRS"),
    ("suffix-hyphens", "prefix_123456789-123456789-123456"),
    ("suffix-wrong-alphabet", "prefix_ooooooiiiiiiuuuuuuulllllll"),
    (
        "suffix-ambiguous-crockford",
        "prefix_i23456789ol23456789oi23456",
    ),
    (
        "suffix-hyphens-crockford",
        "prefix_123456789-0123456789-0123456",
    ),
    ("suffix-overflow", "prefix_8zzzzzzzzzzzzzzzzzzzzzzzzz"),
    (
        "prefix-underscore-start",
        "_prefix_00000000000000000000000000",
    ),
    (
        "prefix-underscore-end",
        "prefix__00000000000000000000000000",
    ),
    ("empty", ""),
    ("prefix-empty", "prefix_"),
];

fn uuid_bytes(text: &str) -> Option<[u8; 16]> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    u128::from_str_radix(&hex, 16).ok().map(u128::to_be_bytes)
}

#[test]
fn typeid_valid_vectors_round_trip() {
    for (name, typeid, prefix, uuid) in VALID {
        let bytes = uuid_bytes(uuid);
        assert!(bytes.is_some(), "{name}: the vector's uuid is hex");
        let bytes = bytes.unwrap_or_default();
        let read = TypeIdText::parse(typeid);
        assert_eq!(
            read.map(|t| t.prefix()),
            Ok(*prefix),
            "{name}: decodes its prefix"
        );
        assert_eq!(
            read.map(|t| *t.value().as_bytes()),
            Ok(bytes),
            "{name}: decodes its uuid"
        );
        let suffix = Opaque128::from_bytes(bytes).to_crockford();
        let encoded = match prefix.is_empty() {
            true => suffix,
            false => format!("{prefix}_{suffix}"),
        };
        assert_eq!(&encoded, typeid, "{name}: encodes to the vector");
    }
}

#[test]
fn typeid_invalid_vectors_are_refused() {
    for (name, typeid) in INVALID {
        assert!(
            TypeIdText::parse(typeid).is_err(),
            "{name}: {typeid} must be refused"
        );
    }
}
