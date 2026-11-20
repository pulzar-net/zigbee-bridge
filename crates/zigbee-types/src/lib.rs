#![doc = include_str!("../README.md")]
#![cfg_attr(
    not(test),
    deny(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )
)]

use std::fmt;
use std::str::FromStr;

pub mod wire;

/// A 16-bit network (short) address.
pub type NodeId = u16;
/// A 16-bit PAN id.
pub type PanId = u16;

/// The coordinator's network address.
pub const COORDINATOR_NODE_ID: NodeId = 0x0000;
/// Broadcast to every device.
pub const BROADCAST_ALL: NodeId = 0xFFFF;
/// Broadcast to every device whose receiver is on when idle.
pub const BROADCAST_RX_ON_WHEN_IDLE: NodeId = 0xFFFD;
/// Broadcast to every router and the coordinator.
pub const BROADCAST_ROUTERS: NodeId = 0xFFFC;

/// Error from parsing a hex address or key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseHexError {
    /// The text was not the expected number of hex digits.
    #[error("expected {expected} hex digits, got {got}")]
    Length {
        /// Digits expected.
        expected: usize,
        /// Digits found.
        got: usize,
    },
    /// A character was not a hex digit.
    #[error("invalid hex digit {0:?}")]
    Digit(char),
}

fn parse_hex_bytes<const N: usize>(text: &str) -> Result<[u8; N], ParseHexError> {
    let text = text.trim();
    let text = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    let digits: Vec<char> = text.chars().filter(|c| *c != ':' && *c != '-').collect();
    if digits.len() != N * 2 {
        return Err(ParseHexError::Length {
            expected: N * 2,
            got: digits.len(),
        });
    }
    let mut out = [0u8; N];
    for (byte, pair) in out.iter_mut().zip(digits.chunks(2)) {
        let mut value = 0u8;
        for c in pair {
            let nibble = c.to_digit(16).ok_or(ParseHexError::Digit(*c))?;
            value = (value << 4) | nibble as u8;
        }
        *byte = value;
    }
    Ok(out)
}

/// A 64-bit IEEE (EUI-64) address.
///
/// Shown and parsed most-significant byte first, as printed on labels and in
/// the open coordinator backup format: `00124b0001020304`. Parsing also
/// accepts a `0x` prefix and `:` separators. On the wire it is little-endian.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Ieee(pub u64);

impl Ieee {
    /// All zeros, used by some stacks to mean "any device".
    pub const ZERO: Ieee = Ieee(0);
    /// All ones, the unset value.
    pub const UNSET: Ieee = Ieee(u64::MAX);

    /// From the 8 wire (little-endian) bytes.
    pub fn from_le_bytes(bytes: [u8; 8]) -> Self {
        Ieee(u64::from_le_bytes(bytes))
    }

    /// The 8 wire (little-endian) bytes.
    pub fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    /// The address as `0x` followed by 16 hex digits, the conventional
    /// friendly name for a device that has none.
    pub fn to_prefixed_hex(self) -> String {
        format!("0x{:016x}", self.0)
    }
}

impl fmt::Display for Ieee {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl fmt::Debug for Ieee {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ieee({:016x})", self.0)
    }
}

impl FromStr for Ieee {
    type Err = ParseHexError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_hex_bytes::<8>(s).map(|b| Ieee(u64::from_be_bytes(b)))
    }
}

/// A 64-bit extended PAN id, shown most-significant byte first like [`Ieee`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ExtPanId(pub u64);

impl ExtPanId {
    /// From the 8 wire (little-endian) bytes.
    pub fn from_le_bytes(bytes: [u8; 8]) -> Self {
        ExtPanId(u64::from_le_bytes(bytes))
    }

    /// The 8 wire (little-endian) bytes.
    pub fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }
}

impl fmt::Display for ExtPanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl fmt::Debug for ExtPanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ExtPanId({:016x})", self.0)
    }
}

impl FromStr for ExtPanId {
    type Err = ParseHexError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_hex_bytes::<8>(s).map(|b| ExtPanId(u64::from_be_bytes(b)))
    }
}

/// A 128-bit AES key (network key, link key, install-code-derived key).
///
/// Its `Debug` output never contains the key, so a key that ends up in a
/// log line by mistake is not leaked. Use [`Key::to_hex`] deliberately.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Key(pub [u8; 16]);

impl Key {
    /// The well-known Zigbee 3.0 / HA "ZigBeeAlliance09" trust center link key.
    pub const ZIGBEE_ALLIANCE_09: Key = Key(*b"ZigBeeAlliance09");

    /// The key as 32 lowercase hex digits, in byte order.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// True when every byte is zero (an unset key).
    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(<redacted>)")
    }
}

impl FromStr for Key {
    type Err = ParseHexError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_hex_bytes::<16>(s).map(Key)
    }
}

#[cfg(feature = "serde")]
mod serde_impls {
    use super::*;
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

    macro_rules! hex_serde {
        ($t:ty, $ser:expr) => {
            impl Serialize for $t {
                fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    let f: fn(&$t) -> String = $ser;
                    s.serialize_str(&f(self))
                }
            }
            impl<'de> Deserialize<'de> for $t {
                fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    let text = String::deserialize(d)?;
                    text.parse().map_err(D::Error::custom)
                }
            }
        };
    }

    hex_serde!(Ieee, |v| v.to_string());
    hex_serde!(ExtPanId, |v| v.to_string());
    hex_serde!(Key, |v| v.to_hex());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ieee_round_trips_in_every_spelling() {
        let a: Ieee = "00124b0001020304".parse().unwrap();
        assert_eq!(a, Ieee(0x00124b0001020304));
        assert_eq!("0x00124B0001020304".parse::<Ieee>().unwrap(), a);
        assert_eq!("00:12:4b:00:01:02:03:04".parse::<Ieee>().unwrap(), a);
        assert_eq!(a.to_string(), "00124b0001020304");
        assert_eq!(a.to_prefixed_hex(), "0x00124b0001020304");
        assert_eq!(a.to_le_bytes(), [4, 3, 2, 1, 0, 0x4b, 0x12, 0]);
        assert!("00124b00010203".parse::<Ieee>().is_err());
        assert!("00124b000102030g".parse::<Ieee>().is_err());
    }

    #[test]
    fn key_debug_is_redacted() {
        let k = Key::ZIGBEE_ALLIANCE_09;
        assert_eq!(format!("{k:?}"), "Key(<redacted>)");
        assert_eq!(k.to_hex(), "5a6967426565416c6c69616e63653039");
        assert_eq!(k.to_hex().parse::<Key>().unwrap(), k);
    }
}
