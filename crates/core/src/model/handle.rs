//! [`Handle`] — the public spelling of an identifier: its kind's prefix and 128 bits.

#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use core::str::FromStr;

use serde::de::{self, Deserialize, Deserializer, Visitor};
use serde::ser::{Serialize, Serializer};

use super::type_id_text::split;
use super::{Bits128, HandleRefusal, HasHandle, Opaque128, Spelled, TypeIdText};

/// The public spelling of a `K` as a TypeID v0.3: `team_01h455vb4pex5vsknk084sn02q`.
///
/// It reveals what the id's bits reveal: a UUIDv7 carries its creation time, so a kind whose creation time is
/// private takes a v4 or v8 id.
pub struct Handle<K> {
    value: Opaque128,
    kind: PhantomData<fn() -> K>,
}

impl<K: HasHandle> Handle<K> {
    /// The handle of `id`.
    #[must_use]
    pub fn of(id: &K) -> Self {
        Self::from_value(Opaque128::from_bytes(id.as_inner().to_bits()))
    }

    /// The id this handle names.
    #[must_use]
    pub fn id(&self) -> K {
        K::from_inner(<K::Inner as Bits128>::from_bits(*self.value.as_bytes()))
    }

    /// Read the one spelling.
    ///
    /// # Errors
    ///
    /// [`HandleRefusal::Prefix`] for a handle of another kind, and the refusals of
    /// [`TypeIdText::parse`] for anything that is not a TypeID.
    pub fn parse(text: &str) -> Result<Self, HandleRefusal> {
        let typeid = TypeIdText::parse(text)?;
        if typeid.prefix() != K::HANDLE_PREFIX.as_str() {
            return Err(HandleRefusal::Prefix {
                expected: K::HANDLE_PREFIX,
            });
        }
        Ok(Self::from_value(typeid.value()))
    }

    /// Read a handle the way a person types one: any case, `i` and `l` as `1`, `o` as `0`.
    ///
    /// # Errors
    ///
    /// As [`parse`](Self::parse), for a text no reading turns into a handle of `K`.
    pub fn read(text: &str) -> Result<Spelled<Self>, HandleRefusal> {
        let (prefix, suffix) = split(text)?;
        let expected = K::HANDLE_PREFIX.as_str();
        if !prefix.eq_ignore_ascii_case(expected) {
            let lowered: String = prefix.chars().map(|c| c.to_ascii_lowercase()).collect();
            return Err(match super::handle_prefix::is_type_id_prefix(&lowered) {
                true => HandleRefusal::Prefix {
                    expected: K::HANDLE_PREFIX,
                },
                false => HandleRefusal::Shape,
            });
        }
        let suffix = Opaque128::read_crockford(suffix).map_err(|_| HandleRefusal::Suffix)?;
        let exact_prefix = prefix == expected;
        Ok(match suffix {
            Spelled::Canonical(value) if exact_prefix => {
                Spelled::Canonical(Self::from_value(value))
            }
            spelled => Spelled::Variant(Self::from_value(spelled.value())),
        })
    }

    /// The one spelling.
    #[must_use]
    pub fn to_text(&self) -> String {
        let prefix = K::HANDLE_PREFIX.as_str();
        let mut text = String::with_capacity(prefix.len() + 1 + Opaque128::LEN);
        if !prefix.is_empty() {
            text.push_str(prefix);
            text.push('_');
        }
        text.push_str(&self.value.to_crockford());
        text
    }
}

impl<K> Handle<K> {
    const fn from_value(value: Opaque128) -> Self {
        Self {
            value,
            kind: PhantomData,
        }
    }
}

impl<K> Clone for Handle<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for Handle<K> {}

impl<K> PartialEq for Handle<K> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<K> Eq for Handle<K> {}

impl<K> PartialOrd for Handle<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K> Ord for Handle<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value.cmp(&other.value)
    }
}

impl<K> Hash for Handle<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl<K: HasHandle> fmt::Display for Handle<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_text())
    }
}

impl<K: HasHandle> fmt::Debug for Handle<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Handle({})", self.to_text())
    }
}

impl<K: HasHandle> FromStr for Handle<K> {
    type Err = HandleRefusal;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl<K: HasHandle> Serialize for Handle<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_text())
    }
}

impl<'de, K: HasHandle> Deserialize<'de> for Handle<K> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(HandleVisitor(PhantomData))
    }
}

/// Reads a handle strictly: a wire value is written by a program, which writes the one spelling.
struct HandleVisitor<K>(PhantomData<fn() -> K>);

impl<K: HasHandle> Visitor<'_> for HandleVisitor<K> {
    type Value = Handle<K>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a `{}_` handle", K::HANDLE_PREFIX)
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
        Handle::parse(text).map_err(E::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::Handle;
    use crate::model::{HandlePrefix, HandleRefusal, HasHandle, Identifier, Spelled};

    #[derive(Clone, Debug, PartialEq)]
    struct TeamId(uuid::Uuid);

    impl Identifier for TeamId {
        type Inner = uuid::Uuid;
        fn from_inner(inner: uuid::Uuid) -> Self {
            Self(inner)
        }
        fn as_inner(&self) -> &uuid::Uuid {
            &self.0
        }
        fn into_inner(self) -> uuid::Uuid {
            self.0
        }
    }

    impl HasHandle for TeamId {
        const HANDLE_PREFIX: HandlePrefix = match HandlePrefix::new("team") {
            Some(prefix) => prefix,
            None => panic!("not a TypeID prefix"),
        };
    }

    #[derive(Clone, Debug, PartialEq)]
    struct BuildId(u128);

    impl Identifier for BuildId {
        type Inner = u128;
        fn from_inner(inner: u128) -> Self {
            Self(inner)
        }
        fn as_inner(&self) -> &u128 {
            &self.0
        }
        fn into_inner(self) -> u128 {
            self.0
        }
    }

    impl HasHandle for BuildId {
        const HANDLE_PREFIX: HandlePrefix = match HandlePrefix::new("build") {
            Some(prefix) => prefix,
            None => panic!("not a TypeID prefix"),
        };
    }

    const SAMPLE: &str = "01h455vb4pex5vsknk084sn02q";

    #[test]
    fn every_uuid_version_and_a_custom_id_round_trip() {
        // One of each UUID version from RFC 9562's examples, plus nil and max.
        let uuids = [
            0_u128,
            u128::MAX,
            0xc232ab00_9414_11ec_b3c8_9f6bdeced846,
            0x000003e8_9414_21ec_8000_9f6bdeced846,
            0x5df41881_3aed_3515_88a7_2f4a814cf09e,
            0x919108f7_52d1_4320_9bac_f847db4148a8,
            0x2ed6657d_e927_568b_95e1_2665a8aea6a2,
            0x1ec9414c_232a_6b00_b3c8_9f6bdeced846,
            0x017f22e2_79b0_7cc3_98c4_dc0c0c07398f,
            0x320c3d4d_cc00_875b_8ec9_32d5f69181c0,
        ]
        .map(uuid::Uuid::from_u128);
        for uuid in uuids {
            let handle = Handle::of(&TeamId(uuid));
            let text = handle.to_text();
            assert!(text.starts_with("team_"), "{text}");
            assert_eq!(text.len(), 31);
            assert_eq!(Handle::<TeamId>::parse(&text), Ok(handle));
            assert_eq!(handle.id(), TeamId(uuid));
        }
        let custom = BuildId(u128::MAX - 7);
        let handle = Handle::of(&custom);
        assert_eq!(
            Handle::<BuildId>::parse(&handle.to_text()).map(|h| h.id()),
            Ok(custom)
        );
    }

    #[test]
    fn lenient_spellings_read_as_a_variant_of_the_canonical_handle() {
        let canonical = Handle::<TeamId>::parse(&format!("team_{SAMPLE}"));
        let canonical = canonical.unwrap_or_else(|_| Handle::of(&TeamId(uuid::Uuid::nil())));
        assert_eq!(
            Handle::<TeamId>::read(&format!("team_{SAMPLE}")),
            Ok(Spelled::Canonical(canonical))
        );
        for variant in [
            format!("TEAM_{}", SAMPLE.to_uppercase()),
            format!("team_{}", SAMPLE.to_uppercase()),
            "team_OIh455vb4pex5vsknkO84sn02q".to_owned(),
            "Team_0Lh455vb4pex5vsknk084sn02q".to_owned(),
        ] {
            assert_eq!(
                Handle::<TeamId>::read(&variant),
                Ok(Spelled::Variant(canonical)),
                "{variant}"
            );
        }
    }

    #[test]
    fn a_prefix_mismatch_is_refused() {
        let expected = TeamId::HANDLE_PREFIX;
        assert_eq!(
            Handle::<TeamId>::parse(&format!("build_{SAMPLE}")),
            Err(HandleRefusal::Prefix { expected })
        );
        assert_eq!(
            Handle::<TeamId>::read(&format!("BUILD_{SAMPLE}")),
            Err(HandleRefusal::Prefix { expected })
        );
        assert_eq!(
            Handle::<TeamId>::parse(SAMPLE),
            Err(HandleRefusal::Prefix { expected })
        );
    }

    #[test]
    fn a_suffix_that_is_not_128_bits_is_refused() {
        for text in [
            "team_8zzzzzzzzzzzzzzzzzzzzzzzzz",
            "team_01h455vb4pex5vsknk084sn0",
            "team_01h455vb4pex5vsknk084snu2q",
        ] {
            assert_eq!(
                Handle::<TeamId>::read(text),
                Err(HandleRefusal::Suffix),
                "{text}"
            );
        }
    }

    #[test]
    fn the_wire_form_is_the_one_spelling() {
        let handle = Handle::of(&TeamId(uuid::Uuid::from_u128(42)));
        let json = serde_json::to_string(&handle).unwrap_or_default();
        assert_eq!(json, format!("\"{}\"", handle.to_text()));
        let back: Result<Handle<TeamId>, _> = serde_json::from_str(&json);
        assert_eq!(back.ok(), Some(handle));
        let upper: Result<Handle<TeamId>, _> = serde_json::from_str(&json.to_uppercase());
        assert!(upper.is_err());
    }
}
