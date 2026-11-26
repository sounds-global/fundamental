use crate::CanonicalError;
use crate::utf16_order::utf16_cmp;
use core::cmp::Ordering;
use serde::ser::{
    Impossible, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};

/// The largest integer every JSON implementation represents exactly (I-JSON, RFC 7493 §2.2).
const MAX_SAFE_INTEGER: i128 = (1 << 53) - 1;

/// A serde serializer that writes the one RFC 8785 encoding of a value.
pub(crate) struct CanonicalWriter<'a> {
    out: &'a mut Vec<u8>,
}

impl<'a> CanonicalWriter<'a> {
    pub(crate) fn new(out: &'a mut Vec<u8>) -> Self {
        Self { out }
    }
}

fn unrepresentable(detail: &str) -> CanonicalError {
    CanonicalError::Unrepresentable {
        detail: detail.to_owned(),
    }
}

fn write_integer(out: &mut Vec<u8>, value: i128) -> Result<(), CanonicalError> {
    if !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&value) {
        return Err(unrepresentable("an integer outside ±(2⁵³−1)"));
    }
    out.extend_from_slice(value.to_string().as_bytes());
    Ok(())
}

/// Write a string exactly as RFC 8785 §3.2.2.2 (ECMAScript `JSON.stringify`) does.
pub fn write_string(out: &mut Vec<u8>, text: &str) {
    out.push(b'"');
    for character in text.chars() {
        match character {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{08}' => out.extend_from_slice(b"\\b"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\u{0C}' => out.extend_from_slice(b"\\f"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\u{00}'..='\u{1F}' => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let code = u32::from(character) as usize;
                out.extend_from_slice(b"\\u00");
                out.push(HEX[code >> 4]);
                out.push(HEX[code & 0xF]);
            }
            other => {
                let mut buffer = [0_u8; 4];
                out.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    out.push(b'"');
}

impl<'a, 'b> Serializer for &'b mut CanonicalWriter<'a> {
    type Ok = ();
    type Error = CanonicalError;
    type SerializeSeq = Array<'b, 'a>;
    type SerializeTuple = Array<'b, 'a>;
    type SerializeTupleStruct = Array<'b, 'a>;
    type SerializeTupleVariant = Array<'b, 'a>;
    type SerializeMap = Object<'b, 'a>;
    type SerializeStruct = Object<'b, 'a>;
    type SerializeStructVariant = Object<'b, 'a>;

    fn serialize_bool(self, value: bool) -> Result<(), CanonicalError> {
        self.out
            .extend_from_slice(if value { b"true" } else { b"false" });
        Ok(())
    }

    fn serialize_i8(self, value: i8) -> Result<(), CanonicalError> {
        write_integer(self.out, value.into())
    }

    fn serialize_i16(self, value: i16) -> Result<(), CanonicalError> {
        write_integer(self.out, value.into())
    }

    fn serialize_i32(self, value: i32) -> Result<(), CanonicalError> {
        write_integer(self.out, value.into())
    }

    fn serialize_i64(self, value: i64) -> Result<(), CanonicalError> {
        write_integer(self.out, value.into())
    }

    fn serialize_i128(self, value: i128) -> Result<(), CanonicalError> {
        write_integer(self.out, value)
    }

    fn serialize_u8(self, value: u8) -> Result<(), CanonicalError> {
        write_integer(self.out, value.into())
    }

    fn serialize_u16(self, value: u16) -> Result<(), CanonicalError> {
        write_integer(self.out, value.into())
    }

    fn serialize_u32(self, value: u32) -> Result<(), CanonicalError> {
        write_integer(self.out, value.into())
    }

    fn serialize_u64(self, value: u64) -> Result<(), CanonicalError> {
        write_integer(self.out, value.into())
    }

    fn serialize_u128(self, value: u128) -> Result<(), CanonicalError> {
        let value =
            i128::try_from(value).map_err(|_| unrepresentable("an integer outside ±(2⁵³−1)"))?;
        write_integer(self.out, value)
    }

    fn serialize_f32(self, _value: f32) -> Result<(), CanonicalError> {
        Err(unrepresentable("a float; decimals travel as strings"))
    }

    fn serialize_f64(self, _value: f64) -> Result<(), CanonicalError> {
        Err(unrepresentable("a float; decimals travel as strings"))
    }

    fn serialize_char(self, value: char) -> Result<(), CanonicalError> {
        let mut buffer = [0_u8; 4];
        write_string(self.out, value.encode_utf8(&mut buffer));
        Ok(())
    }

    fn serialize_str(self, value: &str) -> Result<(), CanonicalError> {
        write_string(self.out, value);
        Ok(())
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<(), CanonicalError> {
        Err(unrepresentable("raw bytes; encode them as a string"))
    }

    fn serialize_none(self) -> Result<(), CanonicalError> {
        self.serialize_unit()
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), CanonicalError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), CanonicalError> {
        self.out.extend_from_slice(b"null");
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), CanonicalError> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<(), CanonicalError> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), CanonicalError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), CanonicalError> {
        self.out.push(b'{');
        write_string(self.out, variant);
        self.out.push(b':');
        value.serialize(&mut *self)?;
        self.out.push(b'}');
        Ok(())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Array<'b, 'a>, CanonicalError> {
        Ok(Array::open(self, None))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Array<'b, 'a>, CanonicalError> {
        Ok(Array::open(self, None))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Array<'b, 'a>, CanonicalError> {
        Ok(Array::open(self, None))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Array<'b, 'a>, CanonicalError> {
        Ok(Array::open(self, Some(variant)))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Object<'b, 'a>, CanonicalError> {
        Ok(Object::open(self, None))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Object<'b, 'a>, CanonicalError> {
        Ok(Object::open(self, None))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Object<'b, 'a>, CanonicalError> {
        Ok(Object::open(self, Some(variant)))
    }
}

pub(crate) struct Array<'b, 'a> {
    writer: &'b mut CanonicalWriter<'a>,
    variant: Option<&'static str>,
    first: bool,
}

impl<'b, 'a> Array<'b, 'a> {
    fn open(writer: &'b mut CanonicalWriter<'a>, variant: Option<&'static str>) -> Self {
        if let Some(name) = variant {
            writer.out.push(b'{');
            write_string(writer.out, name);
            writer.out.push(b':');
        }
        writer.out.push(b'[');
        Self {
            writer,
            variant,
            first: true,
        }
    }

    fn element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CanonicalError> {
        if !self.first {
            self.writer.out.push(b',');
        }
        self.first = false;
        value.serialize(&mut *self.writer)
    }

    fn close(self) -> Result<(), CanonicalError> {
        self.writer.out.push(b']');
        if self.variant.is_some() {
            self.writer.out.push(b'}');
        }
        Ok(())
    }
}

impl SerializeSeq for Array<'_, '_> {
    type Ok = ();
    type Error = CanonicalError;

    fn serialize_element<T: ?Sized + Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), CanonicalError> {
        self.element(value)
    }

    fn end(self) -> Result<(), CanonicalError> {
        self.close()
    }
}

impl SerializeTuple for Array<'_, '_> {
    type Ok = ();
    type Error = CanonicalError;

    fn serialize_element<T: ?Sized + Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), CanonicalError> {
        self.element(value)
    }

    fn end(self) -> Result<(), CanonicalError> {
        self.close()
    }
}

impl SerializeTupleStruct for Array<'_, '_> {
    type Ok = ();
    type Error = CanonicalError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CanonicalError> {
        self.element(value)
    }

    fn end(self) -> Result<(), CanonicalError> {
        self.close()
    }
}

impl SerializeTupleVariant for Array<'_, '_> {
    type Ok = ();
    type Error = CanonicalError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CanonicalError> {
        self.element(value)
    }

    fn end(self) -> Result<(), CanonicalError> {
        self.close()
    }
}

/// An object being written: members buffered, sorted by UTF-16 code unit, a repeated name refused.
pub(crate) struct Object<'b, 'a> {
    writer: &'b mut CanonicalWriter<'a>,
    variant: Option<&'static str>,
    members: Vec<(String, Vec<u8>)>,
    pending_key: Option<String>,
}

impl<'b, 'a> Object<'b, 'a> {
    fn open(writer: &'b mut CanonicalWriter<'a>, variant: Option<&'static str>) -> Self {
        Self {
            writer,
            variant,
            members: Vec::new(),
            pending_key: None,
        }
    }

    fn member<T: ?Sized + Serialize>(
        &mut self,
        name: String,
        value: &T,
    ) -> Result<(), CanonicalError> {
        let mut encoded = Vec::new();
        value.serialize(&mut CanonicalWriter::new(&mut encoded))?;
        self.members.push((name, encoded));
        Ok(())
    }

    fn close(mut self) -> Result<(), CanonicalError> {
        self.members
            .sort_by(|(left, _), (right, _)| utf16_cmp(left, right));
        if self
            .members
            .windows(2)
            .any(|pair| utf16_cmp(&pair[0].0, &pair[1].0) == Ordering::Equal)
        {
            return Err(unrepresentable("an object naming one member twice"));
        }
        let out = &mut *self.writer.out;
        if let Some(name) = self.variant {
            out.push(b'{');
            write_string(out, name);
            out.push(b':');
        }
        out.push(b'{');
        for (index, (name, value)) in self.members.iter().enumerate() {
            if index > 0 {
                out.push(b',');
            }
            write_string(out, name);
            out.push(b':');
            out.extend_from_slice(value);
        }
        out.push(b'}');
        if self.variant.is_some() {
            out.push(b'}');
        }
        Ok(())
    }
}

impl SerializeMap for Object<'_, '_> {
    type Ok = ();
    type Error = CanonicalError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), CanonicalError> {
        self.pending_key = Some(key.serialize(MemberName)?);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CanonicalError> {
        let name = self
            .pending_key
            .take()
            .ok_or_else(|| unrepresentable("a map value with no key"))?;
        self.member(name, value)
    }

    fn end(self) -> Result<(), CanonicalError> {
        self.close()
    }
}

impl SerializeStruct for Object<'_, '_> {
    type Ok = ();
    type Error = CanonicalError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), CanonicalError> {
        self.member(name.to_owned(), value)
    }

    fn end(self) -> Result<(), CanonicalError> {
        self.close()
    }
}

impl SerializeStructVariant for Object<'_, '_> {
    type Ok = ();
    type Error = CanonicalError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), CanonicalError> {
        self.member(name.to_owned(), value)
    }

    fn end(self) -> Result<(), CanonicalError> {
        self.close()
    }
}

struct MemberName;

impl Serializer for MemberName {
    type Ok = String;
    type Error = CanonicalError;
    type SerializeSeq = Impossible<String, CanonicalError>;
    type SerializeTuple = Impossible<String, CanonicalError>;
    type SerializeTupleStruct = Impossible<String, CanonicalError>;
    type SerializeTupleVariant = Impossible<String, CanonicalError>;
    type SerializeMap = Impossible<String, CanonicalError>;
    type SerializeStruct = Impossible<String, CanonicalError>;
    type SerializeStructVariant = Impossible<String, CanonicalError>;

    fn serialize_str(self, value: &str) -> Result<String, CanonicalError> {
        Ok(value.to_owned())
    }

    fn serialize_char(self, value: char) -> Result<String, CanonicalError> {
        Ok(value.to_string())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<String, CanonicalError> {
        Ok(variant.to_owned())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<String, CanonicalError> {
        value.serialize(self)
    }

    fn serialize_bool(self, _value: bool) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_i8(self, _value: i8) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_i16(self, _value: i16) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_i32(self, _value: i32) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_i64(self, _value: i64) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_u8(self, _value: u8) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_u16(self, _value: u16) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_u32(self, _value: u32) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_u64(self, _value: u64) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_f32(self, _value: f32) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_f64(self, _value: f64) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_none(self) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_some<T: ?Sized + Serialize>(self, _value: &T) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_unit(self) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<String, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, CanonicalError> {
        Err(unrepresentable("a non-string member name"))
    }
}
