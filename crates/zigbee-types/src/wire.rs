//! A bounds-checked little-endian reader and writer for frame parsing.
//!
//! [`Reader`] never panics: every read returns [`Truncated`] when the input
//! runs out, so parsers built on it turn malformed input into errors.

use crate::{ExtPanId, Ieee, Key};

/// The input ended before the value being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("frame truncated: needed {needed} more bytes at offset {offset}")]
pub struct Truncated {
    /// Offset of the read that failed.
    pub offset: usize,
    /// Bytes that were missing.
    pub needed: usize,
}

/// A cursor over a byte slice that reads little-endian values.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `buf`.
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    /// Bytes not yet read.
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    /// True when everything has been read.
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Current offset from the start.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// The next `n` bytes.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], Truncated> {
        let end = self.pos.checked_add(n).filter(|end| *end <= self.buf.len());
        match end.and_then(|end| self.buf.get(self.pos..end)) {
            Some(slice) => {
                self.pos += n;
                Ok(slice)
            }
            None => Err(Truncated {
                offset: self.pos,
                needed: n.saturating_sub(self.remaining()),
            }),
        }
    }

    /// Everything not yet read.
    pub fn rest(&mut self) -> &'a [u8] {
        let rest = self.buf.get(self.pos..).unwrap_or(&[]);
        self.pos = self.buf.len();
        rest
    }

    /// A fixed-size array.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], Truncated> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
    }

    /// One byte.
    pub fn u8(&mut self) -> Result<u8, Truncated> {
        Ok(self.array::<1>()?[0])
    }

    /// One signed byte.
    pub fn i8(&mut self) -> Result<i8, Truncated> {
        Ok(self.u8()? as i8)
    }

    /// A little-endian `u16`.
    pub fn u16(&mut self) -> Result<u16, Truncated> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// A little-endian `u32`.
    pub fn u32(&mut self) -> Result<u32, Truncated> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// A little-endian `u64`.
    pub fn u64(&mut self) -> Result<u64, Truncated> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// A little-endian unsigned integer of `width` bytes (1 to 8).
    pub fn uint_n(&mut self, width: usize) -> Result<u64, Truncated> {
        let bytes = self.bytes(width.min(8))?;
        Ok(bytes
            .iter()
            .rev()
            .fold(0u64, |acc, b| (acc << 8) | u64::from(*b)))
    }

    /// A boolean byte (non-zero is true).
    pub fn bool(&mut self) -> Result<bool, Truncated> {
        Ok(self.u8()? != 0)
    }

    /// An IEEE address in wire order.
    pub fn ieee(&mut self) -> Result<Ieee, Truncated> {
        Ok(Ieee::from_le_bytes(self.array()?))
    }

    /// An extended PAN id in wire order.
    pub fn ext_pan_id(&mut self) -> Result<ExtPanId, Truncated> {
        Ok(ExtPanId::from_le_bytes(self.array()?))
    }

    /// A 16-byte key, in byte order.
    pub fn key(&mut self) -> Result<Key, Truncated> {
        Ok(Key(self.array()?))
    }

    /// A byte string prefixed by a one-byte length.
    pub fn u8_prefixed(&mut self) -> Result<&'a [u8], Truncated> {
        let len = self.u8()?;
        self.bytes(usize::from(len))
    }

    /// `count` little-endian `u16` values.
    pub fn u16_list(&mut self, count: usize) -> Result<Vec<u16>, Truncated> {
        // Check the length first so a large count cannot allocate.
        let raw = self.bytes(count.saturating_mul(2))?;
        Ok(raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect())
    }
}

/// Little-endian writers for `Vec<u8>`.
pub trait Writer {
    /// Append one byte.
    fn put_u8(&mut self, v: u8);
    /// Append a little-endian `u16`.
    fn put_u16(&mut self, v: u16);
    /// Append a little-endian `u32`.
    fn put_u32(&mut self, v: u32);
    /// Append a little-endian `u64`.
    fn put_u64(&mut self, v: u64);
    /// Append the low `width` bytes of `v`, little-endian.
    fn put_uint_n(&mut self, v: u64, width: usize);
    /// Append bytes.
    fn put_bytes(&mut self, v: &[u8]);
    /// Append an IEEE address in wire order.
    fn put_ieee(&mut self, v: Ieee);
    /// Append an extended PAN id in wire order.
    fn put_ext_pan_id(&mut self, v: ExtPanId);
    /// Append a key in byte order.
    fn put_key(&mut self, v: &Key);
    /// Append a byte string with a one-byte length prefix, truncated to 255.
    fn put_u8_prefixed(&mut self, v: &[u8]);
}

impl Writer for Vec<u8> {
    fn put_u8(&mut self, v: u8) {
        self.push(v);
    }
    fn put_u16(&mut self, v: u16) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn put_u32(&mut self, v: u32) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn put_u64(&mut self, v: u64) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn put_uint_n(&mut self, v: u64, width: usize) {
        let bytes = v.to_le_bytes();
        self.extend_from_slice(bytes.get(..width.min(8)).unwrap_or(&bytes));
    }
    fn put_bytes(&mut self, v: &[u8]) {
        self.extend_from_slice(v);
    }
    fn put_ieee(&mut self, v: Ieee) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn put_ext_pan_id(&mut self, v: ExtPanId) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn put_key(&mut self, v: &Key) {
        self.extend_from_slice(&v.0);
    }
    fn put_u8_prefixed(&mut self, v: &[u8]) {
        let v = v.get(..v.len().min(255)).unwrap_or(v);
        self.push(v.len() as u8);
        self.extend_from_slice(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_reports_truncation() {
        let mut r = Reader::new(&[1, 2, 3, 4, 5]);
        assert_eq!(r.u16().unwrap(), 0x0201);
        assert_eq!(r.uint_n(3).unwrap(), 0x050403);
        assert_eq!(
            r.u8(),
            Err(Truncated {
                offset: 5,
                needed: 1
            })
        );
        let mut r = Reader::new(&[3, 1]);
        assert!(r.u8_prefixed().is_err());
        let mut r = Reader::new(&[]);
        assert!(r.u16_list(usize::MAX).is_err());
    }

    #[test]
    fn writes_little_endian() {
        let mut w = Vec::new();
        w.put_u16(0x1234);
        w.put_uint_n(0xAABBCC, 3);
        w.put_u8_prefixed(b"hi");
        assert_eq!(w, [0x34, 0x12, 0xCC, 0xBB, 0xAA, 2, b'h', b'i']);
    }
}
