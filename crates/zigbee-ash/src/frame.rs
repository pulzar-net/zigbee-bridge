//! ASH frames: control byte, data randomization, CRC and byte stuffing.

use crc::{Crc, CRC_16_IBM_3740};

/// Flag byte: ends every frame.
pub const FLAG: u8 = 0x7E;
/// Escape byte: the next byte is XORed with [`FLIP`].
pub const ESCAPE: u8 = 0x7D;
/// Software flow control: resume.
pub const XON: u8 = 0x11;
/// Software flow control: pause.
pub const XOFF: u8 = 0x13;
/// Substitute byte: a low-level receive error replaced a byte; the frame is bad.
pub const SUBSTITUTE: u8 = 0x18;
/// Cancel byte: discards the frame in progress.
pub const CANCEL: u8 = 0x1A;
/// XOR applied to an escaped byte.
pub const FLIP: u8 = 0x20;

/// The ASH protocol version this crate speaks.
pub const ASH_VERSION: u8 = 2;

/// Smallest DATA field (the smallest EZSP frame).
pub const MIN_DATA_LEN: usize = 3;
/// Largest DATA field (the largest EZSP frame).
pub const MAX_DATA_LEN: usize = 220;
/// Largest frame before stuffing: control, data and CRC.
pub const MAX_FRAME_LEN: usize = 1 + MAX_DATA_LEN + 2;

/// CRC-CCITT as ASH uses it: polynomial 0x1021, initial 0xFFFF, not reflected.
const CRC: Crc<u16> = Crc::<u16>::new(&CRC_16_IBM_3740);

/// Reset codes an NCP reports in RSTACK and ERROR frames.
pub mod reset_code {
    /// Reset reason unknown.
    pub const UNKNOWN: u8 = 0x00;
    /// External (pin) reset.
    pub const EXTERNAL: u8 = 0x01;
    /// Power on.
    pub const POWER_ON: u8 = 0x02;
    /// Watchdog.
    pub const WATCHDOG: u8 = 0x03;
    /// Assert.
    pub const ASSERT: u8 = 0x06;
    /// Returned from the bootloader.
    pub const BOOTLOADER: u8 = 0x09;
    /// Software reset, the reply to RST.
    pub const SOFTWARE: u8 = 0x0B;
    /// The NCP gave up after too many ACK timeouts.
    pub const ACK_TIMEOUTS: u8 = 0x51;
    /// Chip-specific reset reason.
    pub const CHIP_SPECIFIC: u8 = 0x80;
}

/// One ASH frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// Carries one EZSP frame.
    Data {
        /// This frame's number, 0 to 7.
        frm_num: u8,
        /// Set when this is a retransmission.
        re_tx: bool,
        /// The next frame number the sender expects to receive.
        ack_num: u8,
        /// The EZSP frame, already de-randomized.
        payload: Vec<u8>,
    },
    /// Acknowledges every frame before `ack_num`.
    Ack {
        /// The next frame number expected.
        ack_num: u8,
        /// The sender is not ready to receive DATA.
        not_ready: bool,
    },
    /// Rejects: the frame `ack_num` was missing or bad.
    Nak {
        /// The next frame number expected.
        ack_num: u8,
        /// The sender is not ready to receive DATA.
        not_ready: bool,
    },
    /// Host to NCP: reset.
    Rst,
    /// NCP to host: reset done.
    RstAck {
        /// ASH version, 2.
        version: u8,
        /// Why the NCP reset ([`reset_code`]).
        code: u8,
    },
    /// NCP to host: fatal error, the NCP is now disconnected.
    Error {
        /// ASH version, 2.
        version: u8,
        /// The error ([`reset_code`]).
        code: u8,
    },
}

/// Why a received frame was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// The CRC did not match.
    #[error("bad CRC")]
    BadCrc,
    /// Shorter than the frame type allows.
    #[error("frame too short")]
    TooShort,
    /// Longer than [`MAX_FRAME_LEN`] or the frame type allows.
    #[error("frame too long")]
    TooLong,
    /// A substitute byte marked a UART error inside the frame.
    #[error("UART error inside frame")]
    CommError,
    /// The control byte names no frame type.
    #[error("invalid control byte {0:#04x}")]
    InvalidControl(u8),
}

/// XOR `data` with ASH's pseudo-random sequence (seed 0x42, polynomial 0xB8).
/// Applying it twice restores the input.
pub fn randomize(data: &mut [u8]) {
    let mut rand: u8 = 0x42;
    for byte in data.iter_mut() {
        *byte ^= rand;
        rand = if rand & 1 == 0 {
            rand >> 1
        } else {
            (rand >> 1) ^ 0xB8
        };
    }
}

fn is_reserved(byte: u8) -> bool {
    matches!(byte, FLAG | ESCAPE | XON | XOFF | SUBSTITUTE | CANCEL)
}

impl Frame {
    /// The control byte.
    pub fn control(&self) -> u8 {
        match self {
            Frame::Data {
                frm_num,
                re_tx,
                ack_num,
                ..
            } => ((frm_num & 7) << 4) | (u8::from(*re_tx) << 3) | (ack_num & 7),
            Frame::Ack { ack_num, not_ready } => 0x80 | (u8::from(*not_ready) << 3) | (ack_num & 7),
            Frame::Nak { ack_num, not_ready } => 0xA0 | (u8::from(*not_ready) << 3) | (ack_num & 7),
            Frame::Rst => 0xC0,
            Frame::RstAck { .. } => 0xC1,
            Frame::Error { .. } => 0xC2,
        }
    }

    /// The frame as sent on the wire: stuffed, with its CRC and closing flag.
    /// RST is preceded by a cancel byte so it never joins a partial frame.
    pub fn encode(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(8);
        raw.push(self.control());
        match self {
            Frame::Data { payload, .. } => {
                let start = raw.len();
                raw.extend_from_slice(payload);
                if let Some(data) = raw.get_mut(start..) {
                    randomize(data);
                }
            }
            Frame::RstAck { version, code } | Frame::Error { version, code } => {
                raw.push(*version);
                raw.push(*code);
            }
            _ => {}
        }
        let crc = CRC.checksum(&raw);
        raw.extend_from_slice(&crc.to_be_bytes());

        let mut out = Vec::with_capacity(raw.len() * 2 + 2);
        if matches!(self, Frame::Rst) {
            out.push(CANCEL);
        }
        for byte in raw {
            if is_reserved(byte) {
                out.push(ESCAPE);
                out.push(byte ^ FLIP);
            } else {
                out.push(byte);
            }
        }
        out.push(FLAG);
        out
    }

    /// Parse an unstuffed frame: control byte, fields and CRC, without the flag.
    pub fn parse(raw: &[u8]) -> Result<Frame, FrameError> {
        if raw.len() > MAX_FRAME_LEN {
            return Err(FrameError::TooLong);
        }
        let (body, crc) = match raw.len().checked_sub(2) {
            Some(split) if split >= 1 => raw.split_at(split),
            _ => return Err(FrameError::TooShort),
        };
        let wanted = u16::from_be_bytes(match crc {
            [a, b] => [*a, *b],
            _ => return Err(FrameError::TooShort),
        });
        if CRC.checksum(body) != wanted {
            return Err(FrameError::BadCrc);
        }
        let (control, fields) = match body.split_first() {
            Some((c, f)) => (*c, f),
            None => return Err(FrameError::TooShort),
        };
        let fixed = |len: usize| -> Result<(), FrameError> {
            match fields.len().cmp(&len) {
                std::cmp::Ordering::Less => Err(FrameError::TooShort),
                std::cmp::Ordering::Greater => Err(FrameError::TooLong),
                std::cmp::Ordering::Equal => Ok(()),
            }
        };
        let ack_num = control & 7;
        let not_ready = control & 0x08 != 0;
        if control & 0x80 == 0 {
            if fields.len() < MIN_DATA_LEN {
                return Err(FrameError::TooShort);
            }
            let mut payload = fields.to_vec();
            randomize(&mut payload);
            return Ok(Frame::Data {
                frm_num: (control >> 4) & 7,
                re_tx: control & 0x08 != 0,
                ack_num,
                payload,
            });
        }
        match control & 0xE0 {
            0x80 => fixed(0).map(|_| Frame::Ack { ack_num, not_ready }),
            0xA0 => fixed(0).map(|_| Frame::Nak { ack_num, not_ready }),
            _ => match (control, fields) {
                (0xC0, _) => fixed(0).map(|_| Frame::Rst),
                (0xC1, [version, code]) => Ok(Frame::RstAck {
                    version: *version,
                    code: *code,
                }),
                (0xC2, [version, code]) => Ok(Frame::Error {
                    version: *version,
                    code: *code,
                }),
                (0xC1 | 0xC2, _) => Err(if fields.len() < 2 {
                    FrameError::TooShort
                } else {
                    FrameError::TooLong
                }),
                _ => Err(FrameError::InvalidControl(control)),
            },
        }
    }
}

/// Splits a received byte stream into frames: removes stuffing, honours the
/// cancel and substitute bytes, ignores XON/XOFF, and bounds the buffer at
/// [`MAX_FRAME_LEN`] so a stream without flags cannot grow memory.
#[derive(Debug, Default, Clone)]
pub struct Decoder {
    buf: Vec<u8>,
    escaped: bool,
    error: Option<FrameError>,
}

impl Decoder {
    /// An empty decoder.
    pub fn new() -> Self {
        Decoder {
            buf: Vec::with_capacity(MAX_FRAME_LEN),
            escaped: false,
            error: None,
        }
    }

    /// Drop any partial frame.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.escaped = false;
        self.error = None;
    }

    /// Feed one byte. Returns a frame, or the reason one was dropped, when a
    /// flag byte ends it.
    pub fn push(&mut self, byte: u8) -> Option<Result<Frame, FrameError>> {
        match byte {
            FLAG => {
                let error = self.error.take();
                let escaped = std::mem::replace(&mut self.escaped, false);
                if self.buf.is_empty() && error.is_none() {
                    return None;
                }
                let result = match error {
                    Some(e) => Err(e),
                    None if escaped => Err(FrameError::CommError),
                    None => Frame::parse(&self.buf),
                };
                self.buf.clear();
                Some(result)
            }
            CANCEL => {
                self.reset();
                None
            }
            SUBSTITUTE => {
                self.error = Some(FrameError::CommError);
                None
            }
            XON | XOFF => None,
            ESCAPE => {
                self.escaped = true;
                None
            }
            _ => {
                if self.error.is_some() {
                    return None;
                }
                let value = if std::mem::replace(&mut self.escaped, false) {
                    byte ^ FLIP
                } else {
                    byte
                };
                if self.buf.len() >= MAX_FRAME_LEN {
                    self.error = Some(FrameError::TooLong);
                    self.buf.clear();
                } else {
                    self.buf.push(value);
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(bytes: &[u8]) -> Vec<Result<Frame, FrameError>> {
        let mut d = Decoder::new();
        bytes.iter().filter_map(|b| d.push(*b)).collect()
    }

    // Worked examples from UG101, "ASH Protocol", section 4.
    #[test]
    fn rst_matches_ug101() {
        assert_eq!(Frame::Rst.encode(), [0x1A, 0xC0, 0x38, 0xBC, 0x7E]);
    }

    #[test]
    fn rstack_matches_ug101() {
        let bytes = [0xC1, 0x02, 0x02, 0x9B, 0x7B, 0x7E];
        assert_eq!(
            decode_all(&bytes),
            vec![Ok(Frame::RstAck {
                version: 2,
                code: 2
            })]
        );
        assert_eq!(
            Frame::RstAck {
                version: 2,
                code: 2
            }
            .encode(),
            bytes
        );
    }

    #[test]
    fn ack_and_nak_match_ug101() {
        assert_eq!(
            Frame::Ack {
                ack_num: 1,
                not_ready: false
            }
            .encode(),
            [0x81, 0x60, 0x59, 0x7E]
        );
        assert_eq!(
            Frame::Nak {
                ack_num: 6,
                not_ready: false
            }
            .encode(),
            [0xA6, 0x34, 0xDC, 0x7E]
        );
    }

    #[test]
    fn version_data_frame_matches_ug101() {
        // EZSP "version" command, seq 0, desired protocol version 2.
        let frame = Frame::Data {
            frm_num: 2,
            re_tx: false,
            ack_num: 5,
            payload: vec![0x00, 0x00, 0x00, 0x02],
        };
        assert_eq!(
            frame.encode(),
            [0x25, 0x42, 0x21, 0xA8, 0x56, 0xA6, 0x09, 0x7E]
        );
        assert_eq!(decode_all(&frame.encode()), vec![Ok(frame)]);
    }

    #[test]
    fn randomize_is_an_involution() {
        let mut data: Vec<u8> = (0..=255).collect();
        randomize(&mut data);
        assert_ne!(data, (0..=255).collect::<Vec<u8>>());
        randomize(&mut data);
        assert_eq!(data, (0..=255).collect::<Vec<u8>>());
    }

    #[test]
    fn stuffing_round_trips_reserved_bytes() {
        // Payload chosen so that randomized bytes and CRC hit reserved values.
        for seed in 0u8..=255 {
            let payload = vec![
                seed,
                FLAG ^ 0x42,
                ESCAPE,
                XON,
                XOFF,
                SUBSTITUTE,
                CANCEL,
                0xFF,
            ];
            let frame = Frame::Data {
                frm_num: seed & 7,
                re_tx: seed & 1 == 1,
                ack_num: 3,
                payload,
            };
            let wire = frame.encode();
            assert!(wire[..wire.len() - 1]
                .iter()
                .all(|b| !is_reserved(*b) || *b == ESCAPE));
            assert_eq!(decode_all(&wire), vec![Ok(frame)]);
        }
    }

    #[test]
    fn bad_crc_cancel_substitute_and_overlong_are_handled() {
        let mut wire = Frame::Ack {
            ack_num: 1,
            not_ready: false,
        }
        .encode();
        wire[1] ^= 0x01;
        assert_eq!(decode_all(&wire), vec![Err(FrameError::BadCrc)]);

        // Cancel discards the partial frame before it.
        let mut stream = vec![0x81, 0x60, CANCEL];
        stream.extend(
            Frame::Ack {
                ack_num: 2,
                not_ready: false,
            }
            .encode(),
        );
        assert_eq!(
            decode_all(&stream),
            vec![Ok(Frame::Ack {
                ack_num: 2,
                not_ready: false
            })]
        );

        assert_eq!(
            decode_all(&[0x81, SUBSTITUTE, 0x60, 0x59, FLAG]),
            vec![Err(FrameError::CommError)]
        );

        let mut long = vec![0x01; MAX_FRAME_LEN + 10];
        long.push(FLAG);
        assert_eq!(decode_all(&long), vec![Err(FrameError::TooLong)]);
        assert_eq!(decode_all(&[FLAG, FLAG]), vec![]);
    }
}
