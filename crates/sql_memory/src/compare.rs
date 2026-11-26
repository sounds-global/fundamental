use std::cmp::Ordering;

use fundamental_db::FilterValue;

/// How `left` stands to `right`, or `None` when SQL has no answer (a NULL, or unrelated values).
pub(crate) fn compared(left: &FilterValue, right: &FilterValue) -> Option<Ordering> {
    use FilterValue as V;
    match (left, right) {
        (V::Null, _) | (_, V::Null) => None,
        (V::Bool(a), V::Bool(b)) => Some(a.cmp(b)),
        (V::Int(a), V::Int(b)) => Some(a.cmp(b)),
        (V::Float(a), V::Float(b)) => a.partial_cmp(b),
        #[expect(
            clippy::cast_precision_loss,
            reason = "SQL compares an integer with a float as floats"
        )]
        (V::Int(a), V::Float(b)) => (*a as f64).partial_cmp(b),
        #[expect(
            clippy::cast_precision_loss,
            reason = "SQL compares an integer with a float as floats"
        )]
        (V::Float(a), V::Int(b)) => a.partial_cmp(&(*b as f64)),
        (V::String(a), V::String(b)) => Some(a.as_bytes().cmp(b.as_bytes())),
        (V::String(a) | V::Uuid(a), V::String(b) | V::Uuid(b)) => Some(uuid_text(a, b)),
        (V::Bytes(a), V::Bytes(b)) => Some(a.cmp(b)),
        (V::TimestampTz(a), V::TimestampTz(b)) => Some(a.cmp(b)),
        (V::Timestamp(a), V::Timestamp(b)) => Some(a.cmp(b)),
        (V::TimestampTz(a), V::Timestamp(b)) => Some(a.timestamp_millis().cmp(b)),
        (V::Timestamp(a), V::TimestampTz(b)) => Some(a.cmp(&b.timestamp_millis())),
        (V::Json(a), V::Json(b)) => (a == b).then_some(Ordering::Equal),
        _ => None,
    }
}

/// Whether `left` equals `right`; NULL equals nothing.
pub(crate) fn same(left: &FilterValue, right: &FilterValue) -> bool {
    compared(left, right) == Some(Ordering::Equal)
}

/// UUID texts compared case-insensitively: in a UUID's hex, text order is byte order.
fn uuid_text(left: &str, right: &str) -> Ordering {
    left.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
}
