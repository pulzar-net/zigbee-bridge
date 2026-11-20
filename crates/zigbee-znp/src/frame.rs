//! MT frames on the wire: `0xFE`, length, two command bytes, data, and an
//! XOR check byte over everything after the start byte.
//!
//! The first command byte holds the frame type in its top three bits and
//! the subsystem in the low five; the second is the command id.

use thiserror::Error;

/// Start of frame.
pub const SOF: u8 = 0xFE;
/// The longest data field MT allows.
pub const MAX_DATA: usize = 250;

/// The frame type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Poll (unused by ZNP hosts).
    Poll,
    /// Synchronous request; the NCP answers with an [`Kind::Srsp`].
    Sreq,
    /// Asynchronous request or indication.
    Areq,
    /// Synchronous response.
    Srsp,
}

impl Kind {
    fn bits(self) -> u8 {
        match self {
            Kind::Poll => 0x00,
            Kind::Sreq => 0x20,
            Kind::Areq => 0x40,
            Kind::Srsp => 0x60,
        }
    }

    fn from_bits(b: u8) -> Option<Kind> {
        match b & 0xE0 {
            0x00 => Some(Kind::Poll),
            0x20 => Some(Kind::Sreq),
            0x40 => Some(Kind::Areq),
            0x60 => Some(Kind::Srsp),
            _ => None,
        }
    }
}

/// One MT frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Frame type.
    pub kind: Kind,
    /// Subsystem (see [`crate::ids::subsystem`]).
    pub subsystem: u8,
    /// Command id within the subsystem.
    pub id: u8,
    /// Data, at most [`MAX_DATA`] bytes.
    pub data: Vec<u8>,
}

/// A frame did not encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FrameError {
    /// The data field is longer than [`MAX_DATA`].
    #[error("MT data of {0} bytes is over the 250 byte limit")]
    TooLong(usize),
    /// The subsystem does not fit five bits.
    #[error("subsystem {0:#04x} does not fit five bits")]
    Subsystem(u8),
}

impl Frame {
    /// A frame.
    pub fn new(kind: Kind, subsystem: u8, id: u8, data: Vec<u8>) -> Frame {
        Frame {
            kind,
            subsystem,
            id,
            data,
        }
    }

    /// The two command bytes.
    pub fn command(&self) -> (u8, u8) {
        (self.kind.bits() | (self.subsystem & 0x1F), self.id)
    }

    /// The bytes on the wire.
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        if self.data.len() > MAX_DATA {
            return Err(FrameError::TooLong(self.data.len()));
        }
        if self.subsystem > 0x1F {
            return Err(FrameError::Subsystem(self.subsystem));
        }
        let (cmd0, cmd1) = self.command();
        let mut out = Vec::with_capacity(self.data.len() + 5);
        out.push(SOF);
        out.push(self.data.len() as u8);
        out.push(cmd0);
        out.push(cmd1);
        out.extend_from_slice(&self.data);
        let fcs = out.iter().skip(1).fold(0u8, |a, b| a ^ b);
        out.push(fcs);
        Ok(out)
    }
}

/// What the decoder found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
    /// A frame with a good check byte.
    Frame(Frame),
    /// A frame whose check byte did not match; dropped.
    BadFcs,
}

/// Splits a byte stream into frames, skipping noise between them. Holds at
/// most one frame's bytes.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    /// An empty decoder.
    pub fn new() -> Decoder {
        Decoder::default()
    }

    /// Feed bytes; returns every complete frame in them.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Decoded> {
        let mut out = Vec::new();
        for &b in bytes {
            if self.buf.is_empty() {
                if b == SOF {
                    self.buf.push(b);
                }
                continue;
            }
            if self.buf.len() == 1 && usize::from(b) > MAX_DATA {
                // Not a length; look for the next start byte.
                self.buf.clear();
                if b == SOF {
                    self.buf.push(b);
                }
                continue;
            }
            self.buf.push(b);
            let Some(&len) = self.buf.get(1) else {
                continue;
            };
            let total = usize::from(len) + 5;
            if self.buf.len() < total {
                continue;
            }
            let frame = std::mem::take(&mut self.buf);
            out.push(parse_whole(&frame));
        }
        out
    }
}

fn parse_whole(frame: &[u8]) -> Decoded {
    let body = frame.get(1..frame.len().saturating_sub(1)).unwrap_or(&[]);
    let fcs = frame.last().copied().unwrap_or(0);
    if body.iter().fold(0u8, |a, b| a ^ b) != fcs {
        return Decoded::BadFcs;
    }
    match (body.get(1), body.get(2), body.get(3..)) {
        (Some(&cmd0), Some(&cmd1), Some(data)) => match Kind::from_bits(cmd0) {
            Some(kind) => Decoded::Frame(Frame {
                kind,
                subsystem: cmd0 & 0x1F,
                id: cmd1,
                data: data.to_vec(),
            }),
            None => Decoded::BadFcs,
        },
        _ => Decoded::BadFcs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_sys_ping() {
        // SYS_PING: FE 00 21 01 20.
        let f = Frame::new(Kind::Sreq, 0x01, 0x01, vec![]);
        assert_eq!(f.encode().unwrap(), [0xFE, 0x00, 0x21, 0x01, 0x20]);
    }

    #[test]
    fn decodes_across_chunks_and_noise() {
        let f = Frame::new(Kind::Srsp, 0x01, 0x01, vec![0x79, 0x07]);
        let bytes = f.encode().unwrap();
        let mut d = Decoder::new();
        assert!(d.push(&[0x00, 0x13]).is_empty());
        assert!(d.push(&bytes[..3]).is_empty());
        assert_eq!(d.push(&bytes[3..]), vec![Decoded::Frame(f.clone())]);
        let mut bad = bytes.clone();
        if let Some(last) = bad.last_mut() {
            *last ^= 1;
        }
        bad.extend_from_slice(&bytes);
        assert_eq!(d.push(&bad), vec![Decoded::BadFcs, Decoded::Frame(f)]);
    }

    #[test]
    fn refuses_long_frames() {
        let f = Frame::new(Kind::Sreq, 1, 1, vec![0; 251]);
        assert_eq!(f.encode(), Err(FrameError::TooLong(251)));
        let mut d = Decoder::new();
        assert!(d.push(&[0xFE, 0xFF, 0xFE, 0x00, 0x21, 0x01, 0x20]).len() == 1);
    }
}
