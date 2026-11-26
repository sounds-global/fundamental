//! Canonical byte encoding of a record for signing.

use crate::SyncRecord;

/// Encodes `(table, record)` into the exact bytes a device signs and the server verifies.
///
/// Every field is length-prefixed and the key is a count then each component, even at arity 1,
/// so no two records share bytes.
#[must_use]
pub fn canonical_bytes(table: &str, record: &SyncRecord) -> Vec<u8> {
    let mut out = Vec::new();
    write_field(&mut out, table.as_bytes());
    out.extend_from_slice(&(record.id.len() as u64).to_be_bytes());
    for component in record.id.components() {
        write_field(&mut out, component.as_bytes());
    }
    out.extend_from_slice(&record.meta.updated_at.to_be_bytes());
    out.extend_from_slice(&record.meta.version.to_be_bytes());
    out.push(u8::from(record.meta.tombstone));
    out.extend_from_slice(&(record.payload.len() as u64).to_be_bytes());
    for (key, value) in &record.payload {
        write_field(&mut out, key.as_bytes());
        write_field(&mut out, value.as_bytes());
    }
    out
}

fn write_field(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use fundamental_db::RecordKey;

    use super::*;
    use crate::SyncMeta;

    fn record(id: &str, payload: &[(&str, &str)]) -> SyncRecord {
        let map: BTreeMap<String, String> = payload
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        SyncRecord::new(id, SyncMeta::new(10, 1), map)
    }

    #[test]
    fn is_stable_for_the_same_record() {
        let r = record("n1", &[("a", "1"), ("b", "2")]);
        assert_eq!(canonical_bytes("note", &r), canonical_bytes("note", &r));
    }

    #[test]
    fn payload_key_order_does_not_matter() {
        let r1 = record("n1", &[("a", "1"), ("b", "2")]);
        let r2 = record("n1", &[("b", "2"), ("a", "1")]);
        assert_eq!(canonical_bytes("note", &r1), canonical_bytes("note", &r2));
    }

    #[test]
    fn differs_when_the_table_differs() {
        let r = record("n1", &[("a", "1")]);
        assert_ne!(canonical_bytes("note", &r), canonical_bytes("task", &r));
    }

    #[test]
    fn differs_when_a_value_differs() {
        let r1 = record("n1", &[("a", "1")]);
        let r2 = record("n1", &[("a", "2")]);
        assert_ne!(canonical_bytes("note", &r1), canonical_bytes("note", &r2));
    }

    #[test]
    fn length_prefix_prevents_field_boundary_collisions() {
        // "ab" + "c" must not collide with "a" + "bc".
        let r1 = record("n1", &[("ab", "c")]);
        let r2 = record("n1", &[("a", "bc")]);
        assert_ne!(canonical_bytes("note", &r1), canonical_bytes("note", &r2));
    }

    #[test]
    fn a_composite_key_cannot_be_resplit_into_the_same_bytes() {
        let payload = BTreeMap::new();
        let left = SyncRecord::new(
            RecordKey::composite("a", ["bc"]),
            SyncMeta::new(10, 1),
            payload.clone(),
        );
        let right = SyncRecord::new(
            RecordKey::composite("ab", ["c"]),
            SyncMeta::new(10, 1),
            payload,
        );
        assert_ne!(
            canonical_bytes("note", &left),
            canonical_bytes("note", &right)
        );
    }

    #[test]
    fn arity_is_part_of_the_signed_bytes() {
        let payload = BTreeMap::new();
        let single = SyncRecord::new(
            RecordKey::single("ab"),
            SyncMeta::new(10, 1),
            payload.clone(),
        );
        let composite = SyncRecord::new(
            RecordKey::composite("a", ["b"]),
            SyncMeta::new(10, 1),
            payload,
        );
        assert_ne!(
            canonical_bytes("note", &single),
            canonical_bytes("note", &composite)
        );
    }

    #[test]
    fn differs_when_tombstone_differs() {
        let mut live = record("n1", &[]);
        let mut dead = record("n1", &[]);
        live.meta.tombstone = false;
        dead.meta.tombstone = true;
        assert_ne!(
            canonical_bytes("note", &live),
            canonical_bytes("note", &dead)
        );
    }
}
