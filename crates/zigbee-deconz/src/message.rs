//! Typed requests and responses, both directions, so the client, the
//! simulator and the fuzz targets share one definition of each layout.

use zigbee_primitives::wire::{Reader, Truncated, Writer};
use zigbee_primitives::{Ieee, NodeId};

use crate::frame::Frame;
use crate::ids::command as cmd;

/// A frame did not decode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// Too short.
    #[error(transparent)]
    Truncated(#[from] Truncated),
    /// A command this crate does not know.
    #[error("unknown deCONZ command {0:#04x}")]
    Unknown(u8),
    /// An address mode this crate does not know.
    #[error("unknown deCONZ address mode {0:#04x}")]
    AddressMode(u8),
}

/// An APS address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Address {
    /// A group (mode 1).
    Group(u16),
    /// A network address (mode 2).
    Nwk(NodeId),
    /// An IEEE address (mode 3).
    Ieee(Ieee),
    /// Both (mode 4).
    NwkIeee(NodeId, Ieee),
}

impl Address {
    fn read(r: &mut Reader<'_>) -> Result<Address, DecodeError> {
        Ok(match r.u8()? {
            1 => Address::Group(r.u16()?),
            2 => Address::Nwk(r.u16()?),
            3 => Address::Ieee(r.ieee()?),
            4 => Address::NwkIeee(r.u16()?, r.ieee()?),
            m => return Err(DecodeError::AddressMode(m)),
        })
    }

    fn write(&self, w: &mut Vec<u8>) {
        match self {
            Address::Group(g) => {
                w.put_u8(1);
                w.put_u16(*g);
            }
            Address::Nwk(n) => {
                w.put_u8(2);
                w.put_u16(*n);
            }
            Address::Ieee(i) => {
                w.put_u8(3);
                w.put_ieee(*i);
            }
            Address::NwkIeee(n, i) => {
                w.put_u8(4);
                w.put_u16(*n);
                w.put_ieee(*i);
            }
        }
    }

    /// The network address, when the address has one.
    pub fn nwk(&self) -> Option<NodeId> {
        match self {
            Address::Nwk(n) | Address::NwkIeee(n, _) => Some(*n),
            _ => None,
        }
    }

    /// The IEEE address, when the address has one.
    pub fn ieee(&self) -> Option<Ieee> {
        match self {
            Address::Ieee(i) | Address::NwkIeee(_, i) => Some(*i),
            _ => None,
        }
    }

    fn is_group(&self) -> bool {
        matches!(self, Address::Group(_))
    }
}

/// An APS data request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApsRequest {
    /// Request id, named again in the confirm.
    pub request_id: u8,
    /// Destination.
    pub destination: Address,
    /// Destination endpoint (absent for a group).
    pub destination_endpoint: Option<u8>,
    /// Profile.
    pub profile: u16,
    /// Cluster.
    pub cluster: u16,
    /// Source endpoint.
    pub source_endpoint: u8,
    /// Payload.
    pub asdu: Vec<u8>,
    /// Transmit options ([`crate::ids::tx_option`]).
    pub tx_options: u8,
    /// Radius.
    pub radius: u8,
}

/// An APS data confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApsConfirm {
    /// Device state.
    pub device_state: u8,
    /// The request's id.
    pub request_id: u8,
    /// Destination.
    pub destination: Address,
    /// Destination endpoint (absent for a group).
    pub destination_endpoint: Option<u8>,
    /// Source endpoint.
    pub source_endpoint: u8,
    /// Delivery status: 0 delivered.
    pub status: u8,
}

/// An APS data indication: a received frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApsIndication {
    /// Device state.
    pub device_state: u8,
    /// Destination.
    pub destination: Address,
    /// Destination endpoint.
    pub destination_endpoint: u8,
    /// Source.
    pub source: Address,
    /// Source endpoint.
    pub source_endpoint: u8,
    /// Profile.
    pub profile: u16,
    /// Cluster.
    pub cluster: u16,
    /// Payload.
    pub asdu: Vec<u8>,
    /// Link quality, when reported.
    pub lqi: Option<u8>,
    /// Signal strength, dBm, when reported.
    pub rssi: Option<i8>,
}

/// A request from the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `VERSION`.
    Version,
    /// `READ_PARAMETER`, with a qualifier for keyed parameters (the key
    /// index, or the IEEE address of a link key).
    ReadParameter {
        /// Parameter.
        param: u8,
        /// Qualifier.
        extra: Vec<u8>,
    },
    /// `WRITE_PARAMETER`.
    WriteParameter {
        /// Parameter.
        param: u8,
        /// Value.
        value: Vec<u8>,
    },
    /// `DEVICE_STATE`.
    DeviceState,
    /// `CHANGE_NETWORK_STATE`.
    ChangeNetworkState {
        /// [`crate::ids::net_state`].
        state: u8,
    },
    /// `APS_DATA_REQUEST`.
    ApsDataRequest(Box<ApsRequest>),
    /// `APS_DATA_CONFIRM`: read the next confirm.
    ApsDataConfirm,
    /// `APS_DATA_INDICATION`: read the next received frame.
    ApsDataIndication,
}

impl Request {
    /// The command id.
    pub fn command(&self) -> u8 {
        match self {
            Request::Version => cmd::VERSION,
            Request::ReadParameter { .. } => cmd::READ_PARAMETER,
            Request::WriteParameter { .. } => cmd::WRITE_PARAMETER,
            Request::DeviceState => cmd::DEVICE_STATE,
            Request::ChangeNetworkState { .. } => cmd::CHANGE_NETWORK_STATE,
            Request::ApsDataRequest(_) => cmd::APS_DATA_REQUEST,
            Request::ApsDataConfirm => cmd::APS_DATA_CONFIRM,
            Request::ApsDataIndication => cmd::APS_DATA_INDICATION,
        }
    }

    /// The frame, with sequence number `seq`.
    pub fn encode(&self, seq: u8) -> Frame {
        let mut w = Vec::new();
        match self {
            Request::Version => w.put_u32(0),
            Request::ReadParameter { param, extra } => {
                w.put_u16((1 + extra.len()) as u16);
                w.put_u8(*param);
                w.put_bytes(extra);
            }
            Request::WriteParameter { param, value } => {
                w.put_u16((1 + value.len()) as u16);
                w.put_u8(*param);
                w.put_bytes(value);
            }
            Request::DeviceState => w.put_bytes(&[0, 0, 0]),
            Request::ChangeNetworkState { state } => w.put_u8(*state),
            Request::ApsDataRequest(a) => {
                let mut body = Vec::new();
                body.put_u8(a.request_id);
                body.put_u8(0); // flags
                a.destination.write(&mut body);
                if let (false, Some(ep)) = (a.destination.is_group(), a.destination_endpoint) {
                    body.put_u8(ep);
                }
                body.put_u16(a.profile);
                body.put_u16(a.cluster);
                body.put_u8(a.source_endpoint);
                body.put_u16(a.asdu.len() as u16);
                body.put_bytes(&a.asdu);
                body.put_u8(a.tx_options);
                body.put_u8(a.radius);
                w.put_u16(body.len() as u16);
                w.put_bytes(&body);
            }
            Request::ApsDataConfirm => w.put_u16(0),
            Request::ApsDataIndication => {
                // Flags 1: report link quality and signal strength.
                w.put_u16(1);
                w.put_u8(1);
            }
        }
        Frame::new(self.command(), seq, 0, w)
    }

    /// Decode a request (the simulator's side).
    pub fn decode(frame: &Frame) -> Result<Request, DecodeError> {
        let mut r = Reader::new(&frame.payload);
        Ok(match frame.command {
            cmd::VERSION => Request::Version,
            cmd::READ_PARAMETER => {
                let len = usize::from(r.u16()?);
                let body = r.bytes(len)?;
                let mut b = Reader::new(body);
                Request::ReadParameter {
                    param: b.u8()?,
                    extra: b.rest().to_vec(),
                }
            }
            cmd::WRITE_PARAMETER => {
                let len = usize::from(r.u16()?);
                let body = r.bytes(len)?;
                let mut b = Reader::new(body);
                Request::WriteParameter {
                    param: b.u8()?,
                    value: b.rest().to_vec(),
                }
            }
            cmd::DEVICE_STATE => Request::DeviceState,
            cmd::CHANGE_NETWORK_STATE => Request::ChangeNetworkState { state: r.u8()? },
            cmd::APS_DATA_REQUEST => {
                let len = usize::from(r.u16()?);
                let mut b = Reader::new(r.bytes(len)?);
                let request_id = b.u8()?;
                let _flags = b.u8()?;
                let destination = Address::read(&mut b)?;
                let destination_endpoint = if destination.is_group() {
                    None
                } else {
                    Some(b.u8()?)
                };
                let profile = b.u16()?;
                let cluster = b.u16()?;
                let source_endpoint = b.u8()?;
                let n = usize::from(b.u16()?);
                let asdu = b.bytes(n)?.to_vec();
                Request::ApsDataRequest(Box::new(ApsRequest {
                    request_id,
                    destination,
                    destination_endpoint,
                    profile,
                    cluster,
                    source_endpoint,
                    asdu,
                    tx_options: b.u8()?,
                    radius: b.u8()?,
                }))
            }
            cmd::APS_DATA_CONFIRM => Request::ApsDataConfirm,
            cmd::APS_DATA_INDICATION => Request::ApsDataIndication,
            other => return Err(DecodeError::Unknown(other)),
        })
    }
}

/// A frame from the NCP: a response or an unsolicited notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// `VERSION`: `0xMMmmPP00` (major, minor, platform).
    Version {
        /// The packed firmware version.
        version: u32,
    },
    /// `READ_PARAMETER`.
    ReadParameter {
        /// Parameter.
        param: u8,
        /// Value.
        value: Vec<u8>,
    },
    /// `WRITE_PARAMETER`.
    WriteParameter {
        /// Parameter.
        param: u8,
    },
    /// `DEVICE_STATE`.
    DeviceState {
        /// Device state.
        device_state: u8,
    },
    /// `CHANGE_NETWORK_STATE`.
    ChangeNetworkState {
        /// The requested state.
        state: u8,
    },
    /// `APS_DATA_REQUEST`.
    ApsDataRequest {
        /// Device state.
        device_state: u8,
        /// The request's id.
        request_id: u8,
    },
    /// `APS_DATA_CONFIRM`.
    ApsDataConfirm(Box<ApsConfirm>),
    /// `APS_DATA_INDICATION`.
    ApsDataIndication(Box<ApsIndication>),
    /// `DEVICE_STATE_CHANGED` (unsolicited).
    DeviceStateChanged {
        /// Device state.
        device_state: u8,
    },
    /// `MAC_POLL` (unsolicited): a child polled.
    MacPoll {
        /// The child.
        source: Address,
    },
}

impl Response {
    /// The command id.
    pub fn command(&self) -> u8 {
        match self {
            Response::Version { .. } => cmd::VERSION,
            Response::ReadParameter { .. } => cmd::READ_PARAMETER,
            Response::WriteParameter { .. } => cmd::WRITE_PARAMETER,
            Response::DeviceState { .. } => cmd::DEVICE_STATE,
            Response::ChangeNetworkState { .. } => cmd::CHANGE_NETWORK_STATE,
            Response::ApsDataRequest { .. } => cmd::APS_DATA_REQUEST,
            Response::ApsDataConfirm(_) => cmd::APS_DATA_CONFIRM,
            Response::ApsDataIndication(_) => cmd::APS_DATA_INDICATION,
            Response::DeviceStateChanged { .. } => cmd::DEVICE_STATE_CHANGED,
            Response::MacPoll { .. } => cmd::MAC_POLL,
        }
    }

    /// The device state the frame carries, if it carries one.
    pub fn device_state(&self) -> Option<u8> {
        match self {
            Response::DeviceState { device_state }
            | Response::DeviceStateChanged { device_state }
            | Response::ApsDataRequest { device_state, .. } => Some(*device_state),
            Response::ApsDataConfirm(c) => Some(c.device_state),
            Response::ApsDataIndication(i) => Some(i.device_state),
            _ => None,
        }
    }

    /// Decode a frame from the NCP. A frame with a failure status decodes
    /// to nothing useful; check [`Frame::status`] first.
    pub fn decode(frame: &Frame) -> Result<Response, DecodeError> {
        let mut r = Reader::new(&frame.payload);
        Ok(match frame.command {
            cmd::VERSION => Response::Version { version: r.u32()? },
            cmd::READ_PARAMETER => {
                let len = usize::from(r.u16()?);
                let mut b = Reader::new(r.bytes(len)?);
                Response::ReadParameter {
                    param: b.u8()?,
                    value: b.rest().to_vec(),
                }
            }
            cmd::WRITE_PARAMETER => {
                let _len = r.u16()?;
                Response::WriteParameter { param: r.u8()? }
            }
            cmd::DEVICE_STATE => Response::DeviceState {
                device_state: r.u8()?,
            },
            cmd::CHANGE_NETWORK_STATE => Response::ChangeNetworkState { state: r.u8()? },
            cmd::APS_DATA_REQUEST => {
                let _len = r.u16()?;
                Response::ApsDataRequest {
                    device_state: r.u8()?,
                    request_id: r.u8()?,
                }
            }
            cmd::APS_DATA_CONFIRM => {
                let len = usize::from(r.u16()?);
                let mut b = Reader::new(r.bytes(len)?);
                let device_state = b.u8()?;
                let request_id = b.u8()?;
                let destination = Address::read(&mut b)?;
                let destination_endpoint = if destination.is_group() {
                    None
                } else {
                    Some(b.u8()?)
                };
                Response::ApsDataConfirm(Box::new(ApsConfirm {
                    device_state,
                    request_id,
                    destination,
                    destination_endpoint,
                    source_endpoint: b.u8()?,
                    status: b.u8()?,
                }))
            }
            cmd::APS_DATA_INDICATION => {
                let len = usize::from(r.u16()?);
                let mut b = Reader::new(r.bytes(len)?);
                let device_state = b.u8()?;
                let destination = Address::read(&mut b)?;
                let destination_endpoint = b.u8()?;
                let source = Address::read(&mut b)?;
                let source_endpoint = b.u8()?;
                let profile = b.u16()?;
                let cluster = b.u16()?;
                let n = usize::from(b.u16()?);
                let asdu = b.bytes(n)?.to_vec();
                // Two reserved bytes, link quality, four reserved, RSSI.
                let tail = b.rest();
                Response::ApsDataIndication(Box::new(ApsIndication {
                    device_state,
                    destination,
                    destination_endpoint,
                    source,
                    source_endpoint,
                    profile,
                    cluster,
                    asdu,
                    lqi: tail.get(2).copied(),
                    rssi: tail.get(7).map(|b| *b as i8),
                }))
            }
            cmd::DEVICE_STATE_CHANGED => Response::DeviceStateChanged {
                device_state: r.u8()?,
            },
            cmd::MAC_POLL => {
                let _len = r.u16()?;
                Response::MacPoll {
                    source: Address::read(&mut r)?,
                }
            }
            other => return Err(DecodeError::Unknown(other)),
        })
    }

    /// Encode with sequence number `seq` and status `status` (the
    /// simulator's side).
    pub fn encode(&self, seq: u8, status: u8) -> Frame {
        let mut w = Vec::new();
        let with_len = |w: &mut Vec<u8>, body: Vec<u8>| {
            w.put_u16(body.len() as u16);
            w.put_bytes(&body);
        };
        match self {
            Response::Version { version } => w.put_u32(*version),
            Response::ReadParameter { param, value } => {
                let mut body = vec![*param];
                body.put_bytes(value);
                with_len(&mut w, body);
            }
            Response::WriteParameter { param } => with_len(&mut w, vec![*param]),
            Response::DeviceState { device_state } => w.put_bytes(&[*device_state, 0, 0]),
            Response::ChangeNetworkState { state } => w.put_u8(*state),
            Response::ApsDataRequest {
                device_state,
                request_id,
            } => with_len(&mut w, vec![*device_state, *request_id]),
            Response::ApsDataConfirm(c) => {
                let mut body = vec![c.device_state, c.request_id];
                c.destination.write(&mut body);
                if let (false, Some(ep)) = (c.destination.is_group(), c.destination_endpoint) {
                    body.put_u8(ep);
                }
                body.put_u8(c.source_endpoint);
                body.put_u8(c.status);
                body.put_u32(0);
                with_len(&mut w, body);
            }
            Response::ApsDataIndication(i) => {
                let mut body = vec![i.device_state];
                i.destination.write(&mut body);
                body.put_u8(i.destination_endpoint);
                i.source.write(&mut body);
                body.put_u8(i.source_endpoint);
                body.put_u16(i.profile);
                body.put_u16(i.cluster);
                body.put_u16(i.asdu.len() as u16);
                body.put_bytes(&i.asdu);
                if let Some(lqi) = i.lqi {
                    body.put_bytes(&[0, 0, lqi]);
                    if let Some(rssi) = i.rssi {
                        body.put_bytes(&[0, 0, 0, 0, rssi as u8]);
                    }
                }
                with_len(&mut w, body);
            }
            Response::DeviceStateChanged { device_state } => w.put_bytes(&[*device_state, 0]),
            Response::MacPoll { source } => {
                let mut body = Vec::new();
                source.write(&mut body);
                with_len(&mut w, body);
            }
        }
        Frame::new(self.command(), seq, status, w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_responses_round_trip() {
        let aps = ApsRequest {
            request_id: 7,
            destination: Address::Nwk(0x1234),
            destination_endpoint: Some(1),
            profile: 0x0104,
            cluster: 6,
            source_endpoint: 1,
            asdu: vec![1, 2, 3],
            tx_options: 4,
            radius: 0,
        };
        for r in [
            Request::ReadParameter {
                param: 0x18,
                extra: vec![0],
            },
            Request::ApsDataRequest(Box::new(aps)),
            Request::ApsDataRequest(Box::new(ApsRequest {
                destination: Address::Group(5),
                destination_endpoint: None,
                request_id: 1,
                profile: 0x0104,
                cluster: 6,
                source_endpoint: 1,
                asdu: vec![],
                tx_options: 0,
                radius: 30,
            })),
        ] {
            assert_eq!(Request::decode(&r.encode(3)), Ok(r));
        }
        let ind = Response::ApsDataIndication(Box::new(ApsIndication {
            device_state: 0x22,
            destination: Address::Nwk(0),
            destination_endpoint: 1,
            source: Address::NwkIeee(0x1234, Ieee(9)),
            source_endpoint: 1,
            profile: 0x0104,
            cluster: 0x0402,
            asdu: vec![0x18, 1, 0x0A],
            lqi: Some(200),
            rssi: Some(-60),
        }));
        assert_eq!(Response::decode(&ind.encode(1, 0)), Ok(ind));
    }
}
