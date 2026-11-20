//! The EZSP frame header: sequence, frame control and frame id.
//!
//! Every EZSP exchange starts with a `version` command in the legacy format
//! (one-byte frame control and frame id). Once the NCP has answered with
//! protocol version 8 or later, both sides use the extended format: a
//! two-byte frame control (whose high byte carries the format version, 1)
//! and a two-byte frame id.

use zigbee_primitives::wire::{Reader, Truncated};

/// Which header layout a frame uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `seq, control, frame_id`: only for the first `version` exchange.
    Legacy,
    /// `seq, control_lo, control_hi, frame_id_lo, frame_id_hi`.
    Extended,
}

/// Frame control: set in responses and callbacks (NCP to host).
pub const CONTROL_RESPONSE: u8 = 0x80;
/// Frame control: the NCP ran out of memory since the last response.
pub const CONTROL_OVERFLOW: u8 = 0x01;
/// Frame control: the NCP truncated this response.
pub const CONTROL_TRUNCATED: u8 = 0x02;
/// Frame control: the NCP has callbacks waiting.
pub const CONTROL_PENDING_CALLBACKS: u8 = 0x04;
/// Frame control: a synchronous callback.
pub const CONTROL_SYNC_CALLBACK: u8 = 0x08;
/// Frame control: an asynchronous callback (the UART host's normal case).
pub const CONTROL_ASYNC_CALLBACK: u8 = 0x10;
/// Extended frame control high byte: format version 1, unsecured, unpadded.
pub const CONTROL_HIGH_FORMAT_1: u8 = 0x01;

/// Frame header errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    /// Shorter than a header.
    #[error(transparent)]
    Truncated(#[from] Truncated),
    /// The extended frame control names an unknown format version.
    #[error("unsupported EZSP frame format version {0}")]
    FormatVersion(u8),
    /// The frame is encrypted or padded (EZSP security), which this crate
    /// does not speak.
    #[error("secured or padded EZSP frames are not supported")]
    Secured,
}

/// A parsed frame header and its parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header<'a> {
    /// Sequence number, echoed in the response.
    pub seq: u8,
    /// Frame control, low byte.
    pub control: u8,
    /// Frame id.
    pub frame_id: u16,
    /// Parameters after the header.
    pub params: &'a [u8],
}

impl Header<'_> {
    /// NCP to host.
    pub fn is_response(&self) -> bool {
        self.control & CONTROL_RESPONSE != 0
    }

    /// A callback (sync or async) rather than the response to a command.
    pub fn is_callback(&self) -> bool {
        self.control & (CONTROL_SYNC_CALLBACK | CONTROL_ASYNC_CALLBACK) != 0
    }

    /// The NCP reported running out of memory.
    pub fn overflow(&self) -> bool {
        self.is_response() && self.control & CONTROL_OVERFLOW != 0
    }

    /// The NCP truncated the response.
    pub fn truncated(&self) -> bool {
        self.is_response() && self.control & CONTROL_TRUNCATED != 0
    }
}

/// Parse a frame in the given format.
pub fn parse(frame: &[u8], format: Format) -> Result<Header<'_>, HeaderError> {
    let mut r = Reader::new(frame);
    let seq = r.u8()?;
    let control = r.u8()?;
    let frame_id = match format {
        Format::Legacy => u16::from(r.u8()?),
        Format::Extended => {
            let high = r.u8()?;
            if high & 0x03 != CONTROL_HIGH_FORMAT_1 {
                return Err(HeaderError::FormatVersion(high & 0x03));
            }
            if high & 0xC0 != 0 {
                return Err(HeaderError::Secured);
            }
            r.u16()?
        }
    };
    Ok(Header {
        seq,
        control,
        frame_id,
        params: r.rest(),
    })
}

/// The format of a frame from the host, as the NCP sees it: the legacy
/// `version` command is exactly four bytes with frame id zero.
pub fn detect_command_format(frame: &[u8]) -> Format {
    match frame {
        [_, 0x00, 0x00, _] => Format::Legacy,
        _ => Format::Extended,
    }
}

/// Build a frame.
pub fn encode(format: Format, seq: u8, control: u8, frame_id: u16, params: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + params.len());
    out.push(seq);
    out.push(control);
    match format {
        Format::Legacy => out.push(frame_id as u8),
        Format::Extended => {
            out.push(CONTROL_HIGH_FORMAT_1);
            out.extend_from_slice(&frame_id.to_le_bytes());
        }
    }
    out.extend_from_slice(params);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_and_extended_round_trip() {
        let legacy = encode(Format::Legacy, 0, 0, 0, &[13]);
        assert_eq!(legacy, [0, 0, 0, 13]);
        assert_eq!(detect_command_format(&legacy), Format::Legacy);
        let ext = encode(Format::Extended, 5, 0, 0x0114, &[1, 2]);
        assert_eq!(ext, [5, 0, 1, 0x14, 0x01, 1, 2]);
        assert_eq!(detect_command_format(&ext), Format::Extended);
        let h = parse(&ext, Format::Extended).unwrap();
        assert_eq!((h.seq, h.frame_id, h.params), (5, 0x0114, &[1u8, 2][..]));
        assert!(parse(&[1, 0x80, 0x81, 0, 0], Format::Extended).is_err());
        assert!(parse(&[1], Format::Extended).is_err());
    }
}
