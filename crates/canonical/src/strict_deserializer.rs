use crate::CanonicalError;
use crate::reader::Reader;
use crate::utf16_order::utf16_cmp;
use core::cmp::Ordering;
use serde::de::value::{BorrowedStrDeserializer, StrDeserializer};
use serde::de::{DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor};
use serde::forward_to_deserialize_any;
use std::borrow::Cow;

/// A serde deserializer that reads only canonical bytes, one token at a time, with no DOM.
pub(crate) struct StrictDeserializer<'de> {
    reader: Reader<'de>,
}

impl<'de> StrictDeserializer<'de> {
    pub(crate) const fn new(reader: Reader<'de>) -> Self {
        Self { reader }
    }

    /// Refuse anything after the value just read.
    pub(crate) fn finish(&self) -> Result<(), CanonicalError> {
        self.reader.finish()
    }

    fn open(&mut self, byte: u8) -> Result<(), CanonicalError> {
        self.reader.begin_value()?;
        self.reader.enter()?;
        self.reader.expect(byte, "a container")
    }

    fn close(&mut self, byte: u8, expected: &'static str) -> Result<(), CanonicalError> {
        self.reader.expect(byte, expected)?;
        self.reader.leave();
        Ok(())
    }
}

impl<'de> serde::Deserializer<'de> for &mut StrictDeserializer<'de> {
    type Error = CanonicalError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, CanonicalError> {
        match self.reader.peek() {
            Some(b'{') => {
                self.open(b'{')?;
                let value = visitor.visit_map(Members::new(self))?;
                self.close(b'}', "the end of an object")?;
                Ok(value)
            }
            Some(b'[') => {
                self.open(b'[')?;
                let value = visitor.visit_seq(Elements::new(self))?;
                self.close(b']', "the end of an array")?;
                Ok(value)
            }
            Some(b'"') => {
                self.reader.begin_value()?;
                match self.reader.string()? {
                    Cow::Borrowed(text) => visitor.visit_borrowed_str(text),
                    Cow::Owned(text) => visitor.visit_string(text),
                }
            }
            Some(b't') => {
                self.reader.begin_value()?;
                self.reader.literal(b"true", "true")?;
                visitor.visit_bool(true)
            }
            Some(b'f') => {
                self.reader.begin_value()?;
                self.reader.literal(b"false", "false")?;
                visitor.visit_bool(false)
            }
            Some(b'n') => {
                self.reader.begin_value()?;
                self.reader.literal(b"null", "null")?;
                visitor.visit_unit()
            }
            Some(b'-' | b'0'..=b'9') => {
                self.reader.begin_value()?;
                visitor.visit_i64(self.reader.integer()?)
            }
            Some(b'+') => Err(CanonicalError::NonCanonicalInteger {
                offset: self.reader.offset(),
            }),
            _ => Err(self.reader.unexpected("a value")),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, CanonicalError> {
        if self.reader.peek() == Some(b'n') {
            self.reader.begin_value()?;
            self.reader.literal(b"null", "null")?;
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, CanonicalError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, CanonicalError> {
        match self.reader.peek() {
            Some(b'"') => {
                self.reader.begin_value()?;
                match self.reader.string()? {
                    Cow::Borrowed(text) => visitor.visit_enum(BorrowedStrDeserializer::new(text)),
                    Cow::Owned(text) => visitor.visit_enum(StrDeserializer::new(&text)),
                }
            }
            Some(b'{') => {
                self.open(b'{')?;
                let value = visitor.visit_enum(Variant { de: self })?;
                self.close(b'}', "the end of a one-member object")?;
                Ok(value)
            }
            _ => Err(self.reader.unexpected("a variant")),
        }
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, CanonicalError> {
        self.deserialize_any(visitor)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier
    }
}

/// The members of one object, each name strictly after the one before it.
struct Members<'a, 'de> {
    de: &'a mut StrictDeserializer<'de>,
    previous: Option<Cow<'de, str>>,
    count: u32,
}

impl<'a, 'de> Members<'a, 'de> {
    const fn new(de: &'a mut StrictDeserializer<'de>) -> Self {
        Self {
            de,
            previous: None,
            count: 0,
        }
    }
}

impl<'de> MapAccess<'de> for Members<'_, 'de> {
    type Error = CanonicalError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, CanonicalError> {
        let reader = &mut self.de.reader;
        if reader.peek() == Some(b'}') {
            return Ok(None);
        }
        if self.count > 0 {
            reader.expect(b',', "a comma or the end of an object")?;
        }
        self.count += 1;
        if self.count > reader.limits().max_members {
            return Err(CanonicalError::TooManyMembers {
                offset: reader.offset(),
                max: reader.limits().max_members,
            });
        }
        let offset = reader.offset();
        let name = reader.string()?;
        if let Some(previous) = &self.previous {
            #[cfg(test)]
            reader.note_compared(previous.len() + name.len());
            match utf16_cmp(previous, &name) {
                Ordering::Less => {}
                Ordering::Equal => return Err(CanonicalError::DuplicateKey { offset }),
                Ordering::Greater => return Err(CanonicalError::KeyOrder { offset }),
            }
        }
        reader.expect(b':', "a colon")?;
        let key = match &name {
            Cow::Borrowed(text) => seed.deserialize(BorrowedStrDeserializer::new(text))?,
            Cow::Owned(text) => seed.deserialize(StrDeserializer::new(text))?,
        };
        self.previous = Some(name);
        Ok(Some(key))
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, CanonicalError> {
        seed.deserialize(&mut *self.de)
    }
}

struct Elements<'a, 'de> {
    de: &'a mut StrictDeserializer<'de>,
    count: u32,
}

impl<'a, 'de> Elements<'a, 'de> {
    const fn new(de: &'a mut StrictDeserializer<'de>) -> Self {
        Self { de, count: 0 }
    }
}

impl<'de> SeqAccess<'de> for Elements<'_, 'de> {
    type Error = CanonicalError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, CanonicalError> {
        let reader = &mut self.de.reader;
        if reader.peek() == Some(b']') {
            return Ok(None);
        }
        if self.count > 0 {
            reader.expect(b',', "a comma or the end of an array")?;
        }
        self.count += 1;
        if self.count > reader.limits().max_members {
            return Err(CanonicalError::TooManyMembers {
                offset: reader.offset(),
                max: reader.limits().max_members,
            });
        }
        seed.deserialize(&mut *self.de).map(Some)
    }
}

/// An enum written as a one-member object: `{"Variant": value}`.
struct Variant<'a, 'de> {
    de: &'a mut StrictDeserializer<'de>,
}

impl<'de> EnumAccess<'de> for Variant<'_, 'de> {
    type Error = CanonicalError;
    type Variant = Self;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self), CanonicalError> {
        let name = self.de.reader.string()?;
        self.de.reader.expect(b':', "a colon")?;
        let tag = match &name {
            Cow::Borrowed(text) => seed.deserialize(BorrowedStrDeserializer::new(text))?,
            Cow::Owned(text) => seed.deserialize(StrDeserializer::new(text))?,
        };
        Ok((tag, self))
    }
}

impl<'de> VariantAccess<'de> for Variant<'_, 'de> {
    type Error = CanonicalError;

    fn unit_variant(self) -> Result<(), CanonicalError> {
        Err(<CanonicalError as serde::de::Error>::custom(
            "a unit variant is written as a string, not a one-member object",
        ))
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, CanonicalError> {
        seed.deserialize(&mut *self.de)
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, CanonicalError> {
        serde::Deserializer::deserialize_any(&mut *self.de, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, CanonicalError> {
        serde::Deserializer::deserialize_any(&mut *self.de, visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::StrictDeserializer;
    use crate::reader::Reader;
    use crate::{Limits, Profile};
    use proptest::prelude::*;
    use serde::Deserialize;
    use serde::de::IgnoredAny;

    const GENEROUS: Limits = Limits {
        max_bytes: 1 << 20,
        max_depth: 1 << 16,
        max_values: 1 << 20,
        max_string_bytes: 1 << 20,
        max_members: 1 << 20,
    };

    fn work(bytes: &[u8]) -> u64 {
        let Ok(reader) = Reader::new(bytes, GENEROUS, Profile::Verifier) else {
            return 0;
        };
        let mut deserializer = StrictDeserializer::new(reader);
        let _ = IgnoredAny::deserialize(&mut deserializer);
        deserializer.reader.work()
    }

    /// An object of short keys in ascending order: the most comparisons per byte.
    fn many_keys(members: usize) -> Vec<u8> {
        let body: Vec<String> = (0..members)
            .map(|index| format!("\"k{index:06}\":0"))
            .collect();
        format!("{{{}}}", body.join(",")).into_bytes()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2_000))]

        #[test]
        fn strict_parse_work_is_linear_in_input(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            prop_assert!(work(&bytes) <= 3 * bytes.len() as u64);
        }
    }

    #[test]
    fn strict_parse_work_is_linear_on_its_worst_shape() {
        for members in [10, 1_000, 20_000] {
            let bytes = many_keys(members);
            assert!(work(&bytes) <= 3 * bytes.len() as u64, "{members} members");
        }
    }

    #[test]
    fn a_nested_array_bomb_is_refused_at_the_depth_limit() {
        let bomb = vec![b'['; 1_000_000];
        let limits = Limits {
            max_bytes: bomb.len(),
            ..Limits::DOCUMENT
        };
        let reader =
            Reader::new(&bomb, limits, Profile::Verifier).expect("the length is within the limit");
        let mut deserializer = StrictDeserializer::new(reader);
        let refused = IgnoredAny::deserialize(&mut deserializer);
        assert!(matches!(
            refused,
            Err(crate::CanonicalError::TooDeep {
                offset: 24,
                max: 24
            })
        ));
        assert_eq!(
            deserializer.reader.work(),
            24,
            "nothing past the limit was read"
        );
    }
}
