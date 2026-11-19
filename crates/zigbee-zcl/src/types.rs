//! ZCL data types (ZCL specification, chapter 2.6).

/// A ZCL data type, by its wire id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[allow(missing_docs)]
#[repr(u8)]
pub enum DataType {
    NoData = 0x00,
    Data8 = 0x08,
    Data16 = 0x09,
    Data24 = 0x0A,
    Data32 = 0x0B,
    Data40 = 0x0C,
    Data48 = 0x0D,
    Data56 = 0x0E,
    Data64 = 0x0F,
    Bool = 0x10,
    Map8 = 0x18,
    Map16 = 0x19,
    Map24 = 0x1A,
    Map32 = 0x1B,
    Map40 = 0x1C,
    Map48 = 0x1D,
    Map56 = 0x1E,
    Map64 = 0x1F,
    Uint8 = 0x20,
    Uint16 = 0x21,
    Uint24 = 0x22,
    Uint32 = 0x23,
    Uint40 = 0x24,
    Uint48 = 0x25,
    Uint56 = 0x26,
    Uint64 = 0x27,
    Int8 = 0x28,
    Int16 = 0x29,
    Int24 = 0x2A,
    Int32 = 0x2B,
    Int40 = 0x2C,
    Int48 = 0x2D,
    Int56 = 0x2E,
    Int64 = 0x2F,
    Enum8 = 0x30,
    Enum16 = 0x31,
    Semi = 0x38,
    Single = 0x39,
    Double = 0x3A,
    OctetString = 0x41,
    CharString = 0x42,
    LongOctetString = 0x43,
    LongCharString = 0x44,
    Array = 0x48,
    Struct = 0x4C,
    Set = 0x50,
    Bag = 0x51,
    TimeOfDay = 0xE0,
    Date = 0xE1,
    UtcTime = 0xE2,
    ClusterId = 0xE8,
    AttributeId = 0xE9,
    BacnetOid = 0xEA,
    Ieee = 0xF0,
    SecurityKey = 0xF1,
    Unknown = 0xFF,
}

/// What a data type holds, which decides how it is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// No value.
    NoData,
    /// Raw bits (`dataN`).
    Data,
    /// Boolean.
    Bool,
    /// Bitmap.
    Bitmap,
    /// Unsigned integer, including time and id types.
    Unsigned,
    /// Signed integer.
    Signed,
    /// Enumeration.
    Enum,
    /// Floating point.
    Float,
    /// Octet string (one- or two-byte length).
    Octets,
    /// Character string (one- or two-byte length).
    Chars,
    /// Array, set or bag.
    List,
    /// Structure.
    Struct,
    /// IEEE address.
    Ieee,
    /// 128-bit key.
    Key,
    /// Unknown type.
    Unknown,
}

impl DataType {
    /// From the wire id.
    pub fn from_u8(id: u8) -> Option<DataType> {
        use DataType::*;
        Some(match id {
            0x00 => NoData,
            0x08 => Data8,
            0x09 => Data16,
            0x0A => Data24,
            0x0B => Data32,
            0x0C => Data40,
            0x0D => Data48,
            0x0E => Data56,
            0x0F => Data64,
            0x10 => Bool,
            0x18 => Map8,
            0x19 => Map16,
            0x1A => Map24,
            0x1B => Map32,
            0x1C => Map40,
            0x1D => Map48,
            0x1E => Map56,
            0x1F => Map64,
            0x20 => Uint8,
            0x21 => Uint16,
            0x22 => Uint24,
            0x23 => Uint32,
            0x24 => Uint40,
            0x25 => Uint48,
            0x26 => Uint56,
            0x27 => Uint64,
            0x28 => Int8,
            0x29 => Int16,
            0x2A => Int24,
            0x2B => Int32,
            0x2C => Int40,
            0x2D => Int48,
            0x2E => Int56,
            0x2F => Int64,
            0x30 => Enum8,
            0x31 => Enum16,
            0x38 => Semi,
            0x39 => Single,
            0x3A => Double,
            0x41 => OctetString,
            0x42 => CharString,
            0x43 => LongOctetString,
            0x44 => LongCharString,
            0x48 => Array,
            0x4C => Struct,
            0x50 => Set,
            0x51 => Bag,
            0xE0 => TimeOfDay,
            0xE1 => Date,
            0xE2 => UtcTime,
            0xE8 => ClusterId,
            0xE9 => AttributeId,
            0xEA => BacnetOid,
            0xF0 => Ieee,
            0xF1 => SecurityKey,
            0xFF => Unknown,
            _ => return None,
        })
    }

    /// The wire id.
    pub fn id(self) -> u8 {
        self as u8
    }

    /// The type's class.
    pub fn class(self) -> Class {
        use DataType::*;
        match self {
            NoData => Class::NoData,
            Data8 | Data16 | Data24 | Data32 | Data40 | Data48 | Data56 | Data64 => Class::Data,
            Bool => Class::Bool,
            Map8 | Map16 | Map24 | Map32 | Map40 | Map48 | Map56 | Map64 => Class::Bitmap,
            Uint8 | Uint16 | Uint24 | Uint32 | Uint40 | Uint48 | Uint56 | Uint64 => Class::Unsigned,
            TimeOfDay | Date | UtcTime | ClusterId | AttributeId | BacnetOid => Class::Unsigned,
            Int8 | Int16 | Int24 | Int32 | Int40 | Int48 | Int56 | Int64 => Class::Signed,
            Enum8 | Enum16 => Class::Enum,
            Semi | Single | Double => Class::Float,
            OctetString | LongOctetString => Class::Octets,
            CharString | LongCharString => Class::Chars,
            Array | Set | Bag => Class::List,
            Struct => Class::Struct,
            Ieee => Class::Ieee,
            SecurityKey => Class::Key,
            Unknown => Class::Unknown,
        }
    }

    /// Encoded size of a fixed-size type; `None` for strings, lists and
    /// structures.
    pub fn fixed_size(self) -> Option<usize> {
        use DataType::*;
        Some(match self {
            NoData | Unknown => 0,
            Data8 | Bool | Map8 | Uint8 | Int8 | Enum8 => 1,
            Data16 | Map16 | Uint16 | Int16 | Enum16 | Semi | ClusterId | AttributeId => 2,
            Data24 | Map24 | Uint24 | Int24 => 3,
            Data32 | Map32 | Uint32 | Int32 | Single | TimeOfDay | Date | UtcTime | BacnetOid => 4,
            Data40 | Map40 | Uint40 | Int40 => 5,
            Data48 | Map48 | Uint48 | Int48 => 6,
            Data56 | Map56 | Uint56 | Int56 => 7,
            Data64 | Map64 | Uint64 | Int64 | Double | Ieee => 8,
            SecurityKey => 16,
            OctetString | CharString | LongOctetString | LongCharString | Array | Struct | Set
            | Bag => return None,
        })
    }

    /// Analog types carry a reportable change in reporting configuration;
    /// discrete types do not.
    pub fn is_analog(self) -> bool {
        matches!(self.class(), Class::Unsigned | Class::Signed | Class::Float)
            && !matches!(
                self,
                DataType::ClusterId | DataType::AttributeId | DataType::BacnetOid
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_round_trips() {
        for id in 0..=255u8 {
            if let Some(t) = DataType::from_u8(id) {
                assert_eq!(t.id(), id);
            }
        }
        assert!(DataType::Int16.is_analog());
        assert!(!DataType::Enum8.is_analog());
        assert!(!DataType::ClusterId.is_analog());
        assert_eq!(DataType::Uint48.fixed_size(), Some(6));
    }
}
