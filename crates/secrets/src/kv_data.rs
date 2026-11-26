//! [`KvData`] — the fields of one key-value write, borrowed for the length of the write.

use std::fmt;
use std::io::{self, Write};

use crate::KvCas;
use crate::error::{SecretError, SecretResult};

/// The most fields one key-value write carries.
pub const KV_FIELD_LIMIT: usize = 64;

/// The longest field name, in bytes.
pub const KV_KEY_LIMIT: usize = 256;

/// The fields of one key-value (version 2) write, borrowed from the caller; `Debug` prints names only.
pub struct KvData<'a> {
    fields: Vec<(&'a str, &'a str)>,
}

impl<'a> KvData<'a> {
    /// The fields `fields`, refused whole if a name is empty, too long, holds a control character
    /// or repeats, or if there are none or more than [`KV_FIELD_LIMIT`].
    ///
    /// # Errors
    /// [`SecretError::InvalidConfig`], naming the field name at fault — never a value.
    pub fn new(fields: &[(&'a str, &'a str)]) -> SecretResult<Self> {
        if fields.is_empty() || fields.len() > KV_FIELD_LIMIT {
            return Err(SecretError::InvalidConfig(format!(
                "a key-value write carries 1 to {KV_FIELD_LIMIT} fields, not {}",
                fields.len()
            )));
        }
        for (index, (key, _)) in fields.iter().enumerate() {
            if key.is_empty() || key.len() > KV_KEY_LIMIT || key.chars().any(char::is_control) {
                return Err(SecretError::InvalidConfig(format!(
                    "{key:?} is not a key-value field name"
                )));
            }
            if fields[..index].iter().any(|(earlier, _)| earlier == key) {
                return Err(SecretError::InvalidConfig(format!(
                    "the field {key:?} is named twice"
                )));
            }
        }
        Ok(Self {
            fields: fields.to_vec(),
        })
    }

    /// The field names, in the order given.
    pub fn keys(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.fields.iter().map(|(key, _)| *key)
    }

    /// Writes the request body for these fields under `cas` into `out` without growing it.
    ///
    /// # Errors
    /// [`BodyOverflow`] when the body does not fit.
    pub(crate) fn encode(&self, cas: KvCas, out: &mut Vec<u8>) -> Result<(), BodyOverflow> {
        let mut out = Bounded(out);
        out.put(b"{")?;
        if let Some(cas) = cas.option() {
            out.put(b"\"options\":{\"cas\":")?;
            out.put(cas.to_string().as_bytes())?;
            out.put(b"},")?;
        }
        out.put(b"\"data\":{")?;
        for (index, (key, value)) in self.fields.iter().enumerate() {
            if index > 0 {
                out.put(b",")?;
            }
            serde_json::to_writer(&mut out, key).map_err(|_| BodyOverflow)?;
            out.put(b":")?;
            serde_json::to_writer(&mut out, value).map_err(|_| BodyOverflow)?;
        }
        out.put(b"}}")
    }
}

impl fmt::Debug for KvData<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KvData")
            .field("keys", &self.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// A request body that would not fit its buffer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BodyOverflow;

/// A writer that refuses rather than reallocate, so no unwiped copy of a value is left behind.
struct Bounded<'v>(&'v mut Vec<u8>);

impl Bounded<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<(), BodyOverflow> {
        self.write_all(bytes).map_err(|_| BodyOverflow)
    }
}

impl Write for Bounded<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len() + bytes.len() > self.0.capacity() {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "body limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::KvVersion;

    fn encoded(data: &KvData<'_>, cas: KvCas, capacity: usize) -> Result<Value, BodyOverflow> {
        let mut out = Vec::with_capacity(capacity);
        data.encode(cas, &mut out)?;
        assert!(out.capacity() == capacity, "the buffer never grew");
        Ok(serde_json::from_slice(&out).expect("valid JSON"))
    }

    #[test]
    fn the_body_carries_the_fields_and_the_check_and_set() {
        let data = KvData::new(&[("user", "a\"b"), ("_entry", "e")]).expect("valid");
        assert_eq!(
            encoded(
                &data,
                KvVersion::new(3).map_or(KvCas::Absent, KvCas::At),
                256
            ),
            Ok(json!({"options": {"cas": 3}, "data": {"user": "a\"b", "_entry": "e"}}))
        );
        assert_eq!(
            encoded(&data, KvCas::Absent, 256),
            Ok(json!({"options": {"cas": 0}, "data": {"user": "a\"b", "_entry": "e"}}))
        );
        assert_eq!(
            encoded(&data, KvCas::Unchecked, 256),
            Ok(json!({"data": {"user": "a\"b", "_entry": "e"}}))
        );
    }

    #[test]
    fn a_body_past_the_buffer_is_refused_without_growing_it() {
        let long = "x".repeat(100);
        let data = KvData::new(&[("k", long.as_str())]).expect("valid");
        assert_eq!(encoded(&data, KvCas::Unchecked, 64), Err(BodyOverflow));
    }

    #[test]
    fn a_bad_field_set_is_refused_naming_no_value() {
        let control = "a\nb";
        let long = "k".repeat(KV_KEY_LIMIT + 1);
        let many: Vec<(&str, &str)> = (0..=KV_FIELD_LIMIT).map(|_| ("k", "v")).collect();
        for fields in [
            vec![],
            vec![("", "secret-value")],
            vec![(control, "secret-value")],
            vec![(long.as_str(), "secret-value")],
            vec![("k", "secret-value"), ("k", "secret-value")],
            many,
        ] {
            let refused = KvData::new(&fields).expect_err("refused");
            assert!(matches!(refused, SecretError::InvalidConfig(_)));
            assert!(!refused.to_string().contains("secret-value"));
        }
    }

    #[test]
    fn debug_prints_the_names_and_never_a_value() {
        let data = KvData::new(&[("password", "hunter2")]).expect("valid");
        let printed = format!("{data:?}");
        assert!(printed.contains("password"));
        assert!(!printed.contains("hunter2"));
    }
}
