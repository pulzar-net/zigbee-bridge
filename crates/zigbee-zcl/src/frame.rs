//! The ZCL frame: header and payload.

use zigbee_types::wire::{Reader, Truncated, Writer};

/// Global (profile-wide) or cluster-specific command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// A global command ([`crate::foundation`]).
    Global,
    /// A command defined by the cluster.
    Cluster,
}

/// Which side of the cluster sent the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// From a client to a server (a command a server receives).
    ClientToServer,
    /// From a server to a client (reports, responses, notifications).
    ServerToClient,
}

impl Direction {
    /// The other direction.
    pub fn reverse(self) -> Direction {
        match self {
            Direction::ClientToServer => Direction::ServerToClient,
            Direction::ServerToClient => Direction::ClientToServer,
        }
    }
}

/// A ZCL frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Global or cluster-specific.
    pub frame_type: FrameType,
    /// Manufacturer code, for manufacturer-specific commands.
    pub manufacturer: Option<u16>,
    /// Who sent it.
    pub direction: Direction,
    /// The sender does not want a default response.
    pub disable_default_response: bool,
    /// Transaction sequence number.
    pub seq: u8,
    /// Command id.
    pub command: u8,
}

/// A ZCL frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The header.
    pub header: Header,
    /// The command's payload.
    pub payload: Vec<u8>,
}

/// The frame was too short, or used reserved frame type bits.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// Too short.
    #[error(transparent)]
    Truncated(#[from] Truncated),
    /// Frame type 2 or 3 is reserved.
    #[error("reserved ZCL frame type {0}")]
    ReservedType(u8),
}

impl Frame {
    /// Parse a frame.
    pub fn parse(bytes: &[u8]) -> Result<Frame, FrameError> {
        let mut r = Reader::new(bytes);
        let control = r.u8()?;
        let frame_type = match control & 0x03 {
            0 => FrameType::Global,
            1 => FrameType::Cluster,
            other => return Err(FrameError::ReservedType(other)),
        };
        let manufacturer = if control & 0x04 != 0 {
            Some(r.u16()?)
        } else {
            None
        };
        let direction = if control & 0x08 != 0 {
            Direction::ServerToClient
        } else {
            Direction::ClientToServer
        };
        let seq = r.u8()?;
        let command = r.u8()?;
        Ok(Frame {
            header: Header {
                frame_type,
                manufacturer,
                direction,
                disable_default_response: control & 0x10 != 0,
                seq,
                command,
            },
            payload: r.rest().to_vec(),
        })
    }

    /// Encode the frame.
    pub fn encode(&self) -> Vec<u8> {
        let h = &self.header;
        let mut control = match h.frame_type {
            FrameType::Global => 0,
            FrameType::Cluster => 1,
        };
        if h.manufacturer.is_some() {
            control |= 0x04;
        }
        if h.direction == Direction::ServerToClient {
            control |= 0x08;
        }
        if h.disable_default_response {
            control |= 0x10;
        }
        let mut w = Vec::with_capacity(5 + self.payload.len());
        w.put_u8(control);
        if let Some(m) = h.manufacturer {
            w.put_u16(m);
        }
        w.put_u8(h.seq);
        w.put_u8(h.command);
        w.put_bytes(&self.payload);
        w
    }

    /// A global command frame.
    pub fn global(direction: Direction, seq: u8, command: u8, payload: Vec<u8>) -> Frame {
        Frame {
            header: Header {
                frame_type: FrameType::Global,
                manufacturer: None,
                direction,
                disable_default_response: false,
                seq,
                command,
            },
            payload,
        }
    }

    /// A cluster-specific command frame.
    pub fn cluster(direction: Direction, seq: u8, command: u8, payload: Vec<u8>) -> Frame {
        Frame {
            header: Header {
                frame_type: FrameType::Cluster,
                manufacturer: None,
                direction,
                disable_default_response: false,
                seq,
                command,
            },
            payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_with_manufacturer_code() {
        let mut f = Frame::cluster(Direction::ServerToClient, 7, 0x0A, vec![1, 2]);
        f.header.manufacturer = Some(0x115F);
        f.header.disable_default_response = true;
        let bytes = f.encode();
        assert_eq!(bytes, [0x1D, 0x5F, 0x11, 7, 0x0A, 1, 2]);
        assert_eq!(Frame::parse(&bytes).unwrap(), f);
        assert!(Frame::parse(&[0x02, 0, 0]).is_err());
        assert!(Frame::parse(&[0x04, 0]).is_err());
    }
}
