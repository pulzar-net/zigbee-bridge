//! deCONZ frames on the wire: SLIP-framed (RFC 1055), each frame a command
//! id, sequence number, status, total length, payload, and a 16-bit
//! checksum (the two's complement of the byte sum).

use zigbee_types::wire::{Reader, Truncated};

const END: u8 = 0xC0;
const ESC: u8 = 0xDB;
const ESC_END: u8 = 0xDC;
const ESC_ESC: u8 = 0xDD;
/// The longest frame (before SLIP escaping) the decoder accepts.
pub const MAX_FRAME: usize = 512;

/// One frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Command id ([`crate::ids::command`]).
    pub command: u8,
    /// Sequence number; a response echoes its request's.
    pub seq: u8,
    /// Status ([`crate::ids::status`]); 0 in requests.
    pub status: u8,
    /// Payload.
    pub payload: Vec<u8>,
}

fn checksum(bytes: &[u8]) -> u16 {
    let sum = bytes
        .iter()
        .fold(0u16, |a, b| a.wrapping_add(u16::from(*b)));
    (!sum).wrapping_add(1)
}

impl Frame {
    /// A frame.
    pub fn new(command: u8, seq: u8, status: u8, payload: Vec<u8>) -> Frame {
        Frame {
            command,
            seq,
            status,
            payload,
        }
    }

    /// The frame before SLIP escaping, with its checksum.
    pub fn to_bytes(&self) -> Vec<u8> {
        let len = (self.payload.len() + 5) as u16;
        let mut out = Vec::with_capacity(self.payload.len() + 7);
        out.push(self.command);
        out.push(self.seq);
        out.push(self.status);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out.extend_from_slice(&checksum(&out).to_le_bytes());
        out
    }

    /// Parse a frame (after SLIP decoding), checking its length and
    /// checksum.
    pub fn parse(bytes: &[u8]) -> Result<Frame, FrameError> {
        if bytes.len() < 7 {
            return Err(FrameError::Short(bytes.len()));
        }
        let (body, crc) = bytes.split_at(bytes.len() - 2);
        let crc = u16::from_le_bytes([crc[0], crc[1]]);
        if checksum(body) != crc {
            return Err(FrameError::Checksum);
        }
        let mut r = Reader::new(body);
        let command = r.u8()?;
        let seq = r.u8()?;
        let status = r.u8()?;
        let len = usize::from(r.u16()?);
        if len != body.len() {
            return Err(FrameError::Length(len));
        }
        Ok(Frame {
            command,
            seq,
            status,
            payload: r.rest().to_vec(),
        })
    }

    /// The bytes on the wire: SLIP-escaped, between END bytes.
    pub fn encode(&self) -> Vec<u8> {
        let bytes = self.to_bytes();
        let mut out = Vec::with_capacity(bytes.len() + 4);
        out.push(END);
        for b in bytes {
            match b {
                END => out.extend_from_slice(&[ESC, ESC_END]),
                ESC => out.extend_from_slice(&[ESC, ESC_ESC]),
                b => out.push(b),
            }
        }
        out.push(END);
        out
    }
}

/// A frame did not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// Shorter than a header and checksum.
    #[error("deCONZ frame of {0} bytes is too short")]
    Short(usize),
    /// The checksum did not match.
    #[error("deCONZ frame checksum mismatch")]
    Checksum,
    /// The length field disagrees with the frame.
    #[error("deCONZ frame length field {0} does not match")]
    Length(usize),
    /// Truncated.
    #[error(transparent)]
    Truncated(#[from] Truncated),
}

/// Splits a SLIP byte stream into frames. Holds at most [`MAX_FRAME`]
/// bytes.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
    escaped: bool,
    overflow: bool,
}

impl Decoder {
    /// An empty decoder.
    pub fn new() -> Decoder {
        Decoder::default()
    }

    /// Feed bytes; returns every frame they complete, good or bad.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<Frame, FrameError>> {
        let mut out = Vec::new();
        for &b in bytes {
            match b {
                END => {
                    if !self.buf.is_empty() && !self.overflow {
                        out.push(Frame::parse(&self.buf));
                    }
                    self.buf.clear();
                    self.escaped = false;
                    self.overflow = false;
                }
                ESC if !self.escaped => self.escaped = true,
                b => {
                    let b = match (self.escaped, b) {
                        (true, ESC_END) => END,
                        (true, ESC_ESC) => ESC,
                        (_, b) => b,
                    };
                    self.escaped = false;
                    if self.buf.len() < MAX_FRAME {
                        self.buf.push(b);
                    } else {
                        self.overflow = true;
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_request_bytes() {
        // VERSION, seq 1: 0D 01 00 09 00 00 00 00 00, checksum E9 FF.
        let f = Frame::new(0x0D, 1, 0, vec![0; 4]);
        assert_eq!(
            f.to_bytes(),
            [0x0D, 0x01, 0x00, 0x09, 0x00, 0, 0, 0, 0, 0xE9, 0xFF]
        );
    }

    #[test]
    fn escapes_and_round_trips() {
        let f = Frame::new(0x17, 0xC0, 0, vec![0xC0, 0xDB, 1]);
        let wire = f.encode();
        assert!(!wire[1..wire.len() - 1].contains(&END));
        let mut d = Decoder::new();
        assert_eq!(d.push(&wire[..4]), vec![]);
        assert_eq!(d.push(&wire[4..]), vec![Ok(f)]);
        let mut bad = Frame::new(1, 1, 0, vec![]).to_bytes();
        bad[1] ^= 1;
        let mut wire = vec![END];
        wire.extend_from_slice(&bad);
        wire.push(END);
        assert_eq!(d.push(&wire), vec![Err(FrameError::Checksum)]);
    }
}
