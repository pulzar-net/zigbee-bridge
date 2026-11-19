//! ZCL attribute and field values.
//!
//! Attribute values are self-describing on the wire (a type id precedes
//! them in reports and read responses), so one [`Value`] type and one
//! decoder cover every cluster.

use zigbee_types::wire::{Reader, Truncated, Writer};
use zigbee_types::{Ieee, Key};

use crate::types::{Class, DataType};

/// Nested arrays and structures deeper than this are rejected.
pub const MAX_DEPTH: usize = 4;

/// A value did not decode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValueError {
    /// Too short.
    #[error(transparent)]
    Truncated(#[from] Truncated),
    /// A type id that ZCL does not define.
    #[error("unknown data type {0:#04x}")]
    UnknownType(u8),
    /// Arrays or structures nested deeper than [`MAX_DEPTH`].
    #[error("values nested too deeply")]
    TooDeep,
    /// An array of a zero-size type claimed elements.
    #[error("array of {0:?} with elements")]
    ZeroSizeElements(DataType),
}

/// A decoded ZCL value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// No data.
    NoData,
    /// A boolean, as sent: 0 false, 1 true, 0xFF invalid.
    Bool(u8),
    /// Raw data (`dataN`) or a bitmap (`mapN`).
    Bits(DataType, u64),
    /// An unsigned integer, or a time, date or id type.
    Uint(DataType, u64),
    /// A signed integer.
    Int(DataType, i64),
    /// An enumeration.
    Enum(DataType, u16),
    /// A floating-point value (half, single or double precision).
    Float(DataType, f64),
    /// An octet string; `None` is the invalid (0xFF / 0xFFFF length) value.
    Octets(DataType, Option<Vec<u8>>),
    /// A character string; `None` is the invalid value. Bytes that are not
    /// UTF-8 are replaced.
    Chars(DataType, Option<String>),
    /// An array, set or bag: container type, element type, elements.
    List(DataType, DataType, Option<Vec<Value>>),
    /// A structure.
    Struct(Option<Vec<Value>>),
    /// An IEEE address.
    Ieee(Ieee),
    /// A 128-bit key.
    Key(Key),
}

fn width(t: DataType) -> usize {
    t.fixed_size().unwrap_or(0)
}

fn sign_extend(raw: u64, bits: usize) -> i64 {
    if bits == 0 || bits >= 64 {
        return raw as i64;
    }
    let shift = 64 - bits;
    ((raw << shift) as i64) >> shift
}

impl Value {
    /// An unsigned 8-bit integer.
    pub fn u8(v: u8) -> Value {
        Value::Uint(DataType::Uint8, u64::from(v))
    }
    /// An unsigned 16-bit integer.
    pub fn u16(v: u16) -> Value {
        Value::Uint(DataType::Uint16, u64::from(v))
    }
    /// An unsigned 32-bit integer.
    pub fn u32(v: u32) -> Value {
        Value::Uint(DataType::Uint32, u64::from(v))
    }
    /// A signed 16-bit integer.
    pub fn i16(v: i16) -> Value {
        Value::Int(DataType::Int16, i64::from(v))
    }
    /// A boolean.
    pub fn bool(v: bool) -> Value {
        Value::Bool(u8::from(v))
    }
    /// An 8-bit enumeration.
    pub fn enum8(v: u8) -> Value {
        Value::Enum(DataType::Enum8, u16::from(v))
    }
    /// A 16-bit enumeration.
    pub fn enum16(v: u16) -> Value {
        Value::Enum(DataType::Enum16, v)
    }
    /// An 8-bit bitmap.
    pub fn map8(v: u8) -> Value {
        Value::Bits(DataType::Map8, u64::from(v))
    }
    /// A 16-bit bitmap.
    pub fn map16(v: u16) -> Value {
        Value::Bits(DataType::Map16, u64::from(v))
    }
    /// A character string.
    pub fn string(v: &str) -> Value {
        Value::Chars(DataType::CharString, Some(v.to_string()))
    }

    /// The value's data type.
    pub fn data_type(&self) -> DataType {
        match self {
            Value::NoData => DataType::NoData,
            Value::Bool(_) => DataType::Bool,
            Value::Bits(t, _)
            | Value::Uint(t, _)
            | Value::Int(t, _)
            | Value::Enum(t, _)
            | Value::Float(t, _)
            | Value::Octets(t, _)
            | Value::Chars(t, _)
            | Value::List(t, _, _) => *t,
            Value::Struct(_) => DataType::Struct,
            Value::Ieee(_) => DataType::Ieee,
            Value::Key(_) => DataType::SecurityKey,
        }
    }

    /// Decode a value of type `t` (the type id has already been read).
    pub fn decode(t: DataType, r: &mut Reader<'_>) -> Result<Value, ValueError> {
        Self::decode_at(t, r, 0)
    }

    /// Decode a type id followed by a value.
    pub fn decode_typed(r: &mut Reader<'_>) -> Result<Value, ValueError> {
        let id = r.u8()?;
        let t = DataType::from_u8(id).ok_or(ValueError::UnknownType(id))?;
        Self::decode(t, r)
    }

    fn decode_at(t: DataType, r: &mut Reader<'_>, depth: usize) -> Result<Value, ValueError> {
        if depth > MAX_DEPTH {
            return Err(ValueError::TooDeep);
        }
        Ok(match t.class() {
            Class::NoData | Class::Unknown => Value::NoData,
            Class::Bool => Value::Bool(r.u8()?),
            Class::Data | Class::Bitmap => Value::Bits(t, r.uint_n(width(t))?),
            Class::Unsigned => Value::Uint(t, r.uint_n(width(t))?),
            Class::Signed => Value::Int(t, sign_extend(r.uint_n(width(t))?, width(t) * 8)),
            Class::Enum => Value::Enum(t, r.uint_n(width(t))? as u16),
            Class::Float => Value::Float(
                t,
                match t {
                    DataType::Semi => f64::from(half::f16::from_bits(r.u16()?).to_f32()),
                    DataType::Single => f64::from(f32::from_bits(r.u32()?)),
                    _ => f64::from_bits(r.u64()?),
                },
            ),
            Class::Octets | Class::Chars => {
                let long = matches!(t, DataType::LongOctetString | DataType::LongCharString);
                let len = if long {
                    usize::from(r.u16()?)
                } else {
                    usize::from(r.u8()?)
                };
                let invalid = if long { len == 0xFFFF } else { len == 0xFF };
                let bytes = if invalid {
                    None
                } else {
                    Some(r.bytes(len)?.to_vec())
                };
                if t.class() == Class::Octets {
                    Value::Octets(t, bytes)
                } else {
                    Value::Chars(t, bytes.map(|b| String::from_utf8_lossy(&b).into_owned()))
                }
            }
            Class::List => {
                let id = r.u8()?;
                let element = DataType::from_u8(id).ok_or(ValueError::UnknownType(id))?;
                let count = r.u16()?;
                if count == 0xFFFF {
                    Value::List(t, element, None)
                } else {
                    let count = usize::from(count);
                    if count > 0 && element.fixed_size() == Some(0) {
                        return Err(ValueError::ZeroSizeElements(element));
                    }
                    // Every element takes at least one byte, so the input
                    // bounds the allocation.
                    let mut items = Vec::with_capacity(count.min(r.remaining()));
                    for _ in 0..count {
                        items.push(Self::decode_at(element, r, depth + 1)?);
                    }
                    Value::List(t, element, Some(items))
                }
            }
            Class::Struct => {
                let count = r.u16()?;
                if count == 0xFFFF {
                    Value::Struct(None)
                } else {
                    let count = usize::from(count);
                    let mut items = Vec::with_capacity(count.min(r.remaining()));
                    for _ in 0..count {
                        let id = r.u8()?;
                        let element = DataType::from_u8(id).ok_or(ValueError::UnknownType(id))?;
                        items.push(Self::decode_at(element, r, depth + 1)?);
                    }
                    Value::Struct(Some(items))
                }
            }
            Class::Ieee => Value::Ieee(r.ieee()?),
            Class::Key => Value::Key(r.key()?),
        })
    }

    /// Encode the value without its type id.
    pub fn encode(&self, w: &mut Vec<u8>) {
        match self {
            Value::NoData => {}
            Value::Bool(v) => w.put_u8(*v),
            Value::Bits(t, v) | Value::Uint(t, v) => w.put_uint_n(*v, width(*t)),
            Value::Int(t, v) => w.put_uint_n(*v as u64, width(*t)),
            Value::Enum(t, v) => w.put_uint_n(u64::from(*v), width(*t)),
            Value::Float(t, v) => match t {
                DataType::Semi => w.put_u16(half::f16::from_f64(*v).to_bits()),
                DataType::Single => w.put_u32((*v as f32).to_bits()),
                _ => w.put_u64(v.to_bits()),
            },
            Value::Octets(t, v) => put_string(w, *t, v.as_deref()),
            Value::Chars(t, v) => put_string(w, *t, v.as_deref().map(str::as_bytes)),
            Value::List(_, element, items) => {
                w.put_u8(element.id());
                match items {
                    None => w.put_u16(0xFFFF),
                    Some(items) => {
                        w.put_u16(items.len().min(0xFFFE) as u16);
                        items.iter().take(0xFFFE).for_each(|i| i.encode(w));
                    }
                }
            }
            Value::Struct(items) => match items {
                None => w.put_u16(0xFFFF),
                Some(items) => {
                    w.put_u16(items.len().min(0xFFFE) as u16);
                    for i in items.iter().take(0xFFFE) {
                        w.put_u8(i.data_type().id());
                        i.encode(w);
                    }
                }
            },
            Value::Ieee(v) => w.put_ieee(*v),
            Value::Key(v) => w.put_key(v),
        }
    }

    /// Encode the type id and the value.
    pub fn encode_typed(&self, w: &mut Vec<u8>) {
        w.put_u8(self.data_type().id());
        self.encode(w);
    }

    /// True for the type's "invalid" (non-value) encoding: a sensor that
    /// has no reading reports this.
    pub fn is_invalid(&self) -> bool {
        match self {
            Value::Bool(v) => *v == 0xFF,
            Value::Uint(t, v) => {
                let bits = width(*t) * 8;
                bits > 0 && *v == (u64::MAX >> (64 - bits))
            }
            Value::Int(t, v) => {
                let bits = width(*t) * 8;
                bits > 0 && *v == sign_extend(1u64 << (bits - 1), bits)
            }
            Value::Enum(t, v) => *v == if *t == DataType::Enum8 { 0xFF } else { 0xFFFF },
            Value::Float(_, v) => v.is_nan(),
            Value::Octets(_, v) => v.is_none(),
            Value::Chars(_, v) => v.is_none(),
            Value::List(_, _, v) => v.is_none(),
            Value::Struct(v) => v.is_none(),
            Value::Ieee(v) => *v == Ieee::UNSET,
            Value::NoData | Value::Bits(..) | Value::Key(_) => false,
        }
    }

    /// The value as a number, unless it is invalid or not numeric.
    pub fn as_f64(&self) -> Option<f64> {
        if self.is_invalid() {
            return None;
        }
        match self {
            Value::Bool(v) => Some(f64::from(*v)),
            Value::Bits(_, v) | Value::Uint(_, v) => Some(*v as f64),
            Value::Int(_, v) => Some(*v as f64),
            Value::Enum(_, v) => Some(f64::from(*v)),
            Value::Float(_, v) => Some(*v),
            _ => None,
        }
    }

    /// The value as an unsigned integer, unless invalid or not integral.
    pub fn as_u64(&self) -> Option<u64> {
        if self.is_invalid() {
            return None;
        }
        match self {
            Value::Bool(v) => Some(u64::from(*v)),
            Value::Bits(_, v) | Value::Uint(_, v) => Some(*v),
            Value::Enum(_, v) => Some(u64::from(*v)),
            Value::Int(_, v) => u64::try_from(*v).ok(),
            _ => None,
        }
    }

    /// The value as a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(0) => Some(false),
            Value::Bool(1) => Some(true),
            _ => None,
        }
    }

    /// The value as text, for character strings.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Chars(_, Some(s)) => Some(s),
            _ => None,
        }
    }
}

fn put_string(w: &mut Vec<u8>, t: DataType, v: Option<&[u8]>) {
    let long = matches!(t, DataType::LongOctetString | DataType::LongCharString);
    match (v, long) {
        (None, false) => w.put_u8(0xFF),
        (None, true) => w.put_u16(0xFFFF),
        (Some(b), false) => w.put_u8_prefixed(b),
        (Some(b), true) => {
            let b = b.get(..b.len().min(0xFFFE)).unwrap_or(b);
            w.put_u16(b.len() as u16);
            w.put_bytes(b);
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        if self.is_invalid() {
            return s.serialize_none();
        }
        match self {
            Value::NoData => s.serialize_none(),
            Value::Bool(v) => s.serialize_bool(*v != 0),
            Value::Bits(_, v) | Value::Uint(_, v) => s.serialize_u64(*v),
            Value::Int(_, v) => s.serialize_i64(*v),
            Value::Enum(_, v) => s.serialize_u16(*v),
            Value::Float(_, v) => s.serialize_f64(*v),
            Value::Octets(_, Some(v)) => {
                s.serialize_str(&v.iter().map(|b| format!("{b:02x}")).collect::<String>())
            }
            Value::Chars(_, Some(v)) => s.serialize_str(v),
            Value::List(_, _, Some(items)) | Value::Struct(Some(items)) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for i in items {
                    seq.serialize_element(i)?;
                }
                seq.end()
            }
            Value::Ieee(v) => s.serialize_str(&v.to_prefixed_hex()),
            Value::Key(v) => s.serialize_str(&v.to_hex()),
            _ => s.serialize_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(v: Value) {
        let mut w = Vec::new();
        v.encode_typed(&mut w);
        let mut r = Reader::new(&w);
        assert_eq!(Value::decode_typed(&mut r).unwrap(), v);
        assert!(r.is_empty());
    }

    #[test]
    fn every_class_round_trips() {
        round_trip(Value::NoData);
        round_trip(Value::bool(true));
        round_trip(Value::Bits(DataType::Map24, 0xABCDEF));
        round_trip(Value::Uint(DataType::Uint48, 0x0102_0304_0506));
        round_trip(Value::Int(DataType::Int24, -2));
        round_trip(Value::i16(-12345));
        round_trip(Value::enum16(0x0015));
        round_trip(Value::Float(DataType::Semi, 1.5));
        round_trip(Value::Float(DataType::Single, -0.25));
        round_trip(Value::Float(DataType::Double, 1e300));
        round_trip(Value::Octets(DataType::OctetString, Some(vec![1, 2, 3])));
        round_trip(Value::Octets(DataType::LongOctetString, None));
        round_trip(Value::string("lumi.weather"));
        round_trip(Value::Chars(DataType::CharString, None));
        round_trip(Value::List(
            DataType::Array,
            DataType::Uint16,
            Some(vec![Value::u16(1), Value::u16(2)]),
        ));
        round_trip(Value::Struct(Some(vec![Value::u8(1), Value::string("x")])));
        round_trip(Value::Ieee(Ieee(0x00124B0012345678)));
        round_trip(Value::Key(Key([9; 16])));
    }

    #[test]
    fn signed_values_sign_extend() {
        let mut r = Reader::new(&[0xFE, 0xFF, 0xFF]);
        assert_eq!(
            Value::decode(DataType::Int24, &mut r).unwrap(),
            Value::Int(DataType::Int24, -2)
        );
    }

    #[test]
    fn invalid_values_are_recognized() {
        assert!(Value::Int(DataType::Int16, -32768).is_invalid());
        assert!(!Value::Int(DataType::Int16, -32767).is_invalid());
        assert!(Value::u16(0xFFFF).is_invalid());
        assert!(Value::Bool(0xFF).is_invalid());
        assert_eq!(Value::u16(0xFFFF).as_f64(), None);
        assert_eq!(Value::i16(2150).as_f64(), Some(2150.0));
    }

    #[test]
    fn hostile_lists_do_not_allocate_or_recurse_forever() {
        // An array of 65534 no-data elements.
        assert_eq!(
            Value::decode_typed(&mut Reader::new(&[0x48, 0x00, 0xFE, 0xFF])),
            Err(ValueError::ZeroSizeElements(DataType::NoData))
        );
        // A claimed 65534 uint8 elements with three bytes present.
        assert!(Value::decode_typed(&mut Reader::new(&[0x48, 0x20, 0xFE, 0xFF, 1, 2, 3])).is_err());
        // Arrays nested beyond the limit.
        let mut deep = Vec::new();
        for _ in 0..10 {
            deep.extend_from_slice(&[0x48, 0x48, 0x01, 0x00]);
        }
        assert!(Value::decode_typed(&mut Reader::new(&deep)).is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serializes_to_plain_json() {
        assert_eq!(serde_json::to_string(&Value::i16(2150)).unwrap(), "2150");
        assert_eq!(serde_json::to_string(&Value::u16(0xFFFF)).unwrap(), "null");
        assert_eq!(serde_json::to_string(&Value::string("a")).unwrap(), "\"a\"");
        assert_eq!(serde_json::to_string(&Value::bool(false)).unwrap(), "false");
    }
}
