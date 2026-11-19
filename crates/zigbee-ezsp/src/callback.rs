//! Typed EZSP callbacks (NCP to host, unsolicited).

use zigbee_types::wire::{Reader, Writer};
use zigbee_types::{Ieee, NodeId};

use crate::command::DecodeError;
use crate::ids::frame as id;
use crate::types::{ApsFrame, ProtocolVersion, RxPacketInfo, Status};

/// A callback from the NCP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callback {
    /// The network came up, went down, opened or closed for joining.
    StackStatus {
        /// The new state.
        status: Status,
    },
    /// The outcome of a `sendUnicast` or `sendBroadcast`.
    MessageSent {
        /// Delivery status.
        status: Status,
        /// [`crate::ids::outgoing`] type.
        kind: u8,
        /// Destination address or table index.
        destination: NodeId,
        /// The APS frame sent.
        aps: ApsFrame,
        /// The tag given when sending.
        tag: u16,
    },
    /// A message for the host.
    IncomingMessage {
        /// [`crate::ids::incoming`] type.
        kind: u8,
        /// The APS frame.
        aps: ApsFrame,
        /// Sender and link quality.
        info: RxPacketInfo,
        /// The APS payload (ZDO or ZCL frame).
        payload: Vec<u8>,
    },
    /// A device joined, rejoined or left (trust center view).
    TrustCenterJoin {
        /// Its network address.
        node_id: NodeId,
        /// Its IEEE address.
        ieee: Ieee,
        /// [`crate::ids::device_update`].
        status: u8,
        /// [`crate::ids::join_decision`] the trust center took.
        decision: u8,
        /// Its parent's network address.
        parent: NodeId,
    },
    /// A child of the coordinator joined or left.
    ChildJoin {
        /// Child table index.
        index: u8,
        /// Joined (true) or left.
        joining: bool,
        /// Its network address.
        node_id: NodeId,
        /// Its IEEE address.
        ieee: Ieee,
        /// [`crate::ids::node_type`].
        node_type: u8,
    },
    /// A route record: the path a device's message took to the coordinator.
    IncomingRouteRecord {
        /// The originator.
        source: NodeId,
        /// The originator's IEEE address.
        ieee: Ieee,
        /// Link quality of the last hop.
        last_hop_lqi: u8,
        /// Signal strength of the last hop, dBm.
        last_hop_rssi: i8,
        /// Relays, from the one nearest the source to the one nearest the
        /// coordinator.
        relays: Vec<NodeId>,
    },
    /// Key establishment with a partner progressed or failed.
    ZigbeeKeyEstablishment {
        /// The partner.
        partner: Ieee,
        /// [`crate::ids::key_status`].
        status: u8,
    },
    /// One channel's result of an energy scan.
    EnergyScanResult {
        /// Channel.
        channel: u8,
        /// Strongest signal heard, dBm.
        max_rssi: i8,
    },
    /// A scan finished (or failed on `channel`).
    ScanComplete {
        /// Channel that failed, or the last channel.
        channel: u8,
        /// Outcome.
        status: Status,
    },
    /// Two devices share a network address.
    IdConflict {
        /// The address.
        node_id: NodeId,
    },
    /// A route error arrived.
    IncomingRouteError {
        /// The error.
        status: Status,
        /// The unreachable target.
        target: NodeId,
    },
    /// A network status command arrived.
    IncomingNetworkStatus {
        /// The network status code.
        code: u8,
        /// Its target.
        target: NodeId,
    },
    /// A concentrator's many-to-one route request was heard.
    IncomingManyToOneRouteRequest {
        /// The concentrator.
        source: NodeId,
        /// Its IEEE address.
        ieee: Ieee,
        /// Path cost.
        cost: u8,
    },
    /// A stack token changed.
    StackTokenChanged {
        /// The token's address.
        token: u16,
    },
    /// The NCP had no callbacks waiting.
    NoCallbacks,
    /// A callback this crate does not decode, passed through.
    Other {
        /// Frame id.
        frame_id: u16,
        /// Raw parameters.
        params: Vec<u8>,
    },
}

impl Callback {
    /// The frame id.
    pub fn frame_id(&self) -> u16 {
        match self {
            Callback::StackStatus { .. } => id::STACK_STATUS_HANDLER,
            Callback::MessageSent { .. } => id::MESSAGE_SENT_HANDLER,
            Callback::IncomingMessage { .. } => id::INCOMING_MESSAGE_HANDLER,
            Callback::TrustCenterJoin { .. } => id::TRUST_CENTER_JOIN_HANDLER,
            Callback::ChildJoin { .. } => id::CHILD_JOIN_HANDLER,
            Callback::IncomingRouteRecord { .. } => id::INCOMING_ROUTE_RECORD_HANDLER,
            Callback::ZigbeeKeyEstablishment { .. } => id::ZIGBEE_KEY_ESTABLISHMENT_HANDLER,
            Callback::EnergyScanResult { .. } => id::ENERGY_SCAN_RESULT_HANDLER,
            Callback::ScanComplete { .. } => id::SCAN_COMPLETE_HANDLER,
            Callback::IdConflict { .. } => id::ID_CONFLICT_HANDLER,
            Callback::IncomingRouteError { .. } => id::INCOMING_ROUTE_ERROR_HANDLER,
            Callback::IncomingNetworkStatus { .. } => id::INCOMING_NETWORK_STATUS_HANDLER,
            Callback::IncomingManyToOneRouteRequest { .. } => {
                id::INCOMING_MANY_TO_ONE_ROUTE_REQUEST_HANDLER
            }
            Callback::StackTokenChanged { .. } => id::STACK_TOKEN_CHANGED_HANDLER,
            Callback::NoCallbacks => id::NO_CALLBACKS,
            Callback::Other { frame_id, .. } => *frame_id,
        }
    }

    /// Decode a callback. Unknown frame ids become [`Callback::Other`];
    /// malformed known ones are errors.
    pub fn decode(
        v: ProtocolVersion,
        frame_id: u16,
        params: &[u8],
    ) -> Result<Callback, DecodeError> {
        let mut r = Reader::new(params);
        Ok(match frame_id {
            id::STACK_STATUS_HANDLER => Callback::StackStatus {
                status: Status::read(&mut r, v)?,
            },
            id::MESSAGE_SENT_HANDLER => {
                if v.sl_status() {
                    let status = Status::read_sl(&mut r)?;
                    Callback::MessageSent {
                        status,
                        kind: r.u8()?,
                        destination: r.u16()?,
                        aps: ApsFrame::read(&mut r)?,
                        tag: r.u16()?,
                    }
                } else {
                    let kind = r.u8()?;
                    let destination = r.u16()?;
                    let aps = ApsFrame::read(&mut r)?;
                    let tag = u16::from(r.u8()?);
                    Callback::MessageSent {
                        status: Status::read(&mut r, v)?,
                        kind,
                        destination,
                        aps,
                        tag,
                    }
                }
            }
            id::INCOMING_MESSAGE_HANDLER => {
                let kind = r.u8()?;
                let aps = ApsFrame::read(&mut r)?;
                let info = if v.sl_status() {
                    RxPacketInfo::read(&mut r)?
                } else {
                    let last_hop_lqi = r.u8()?;
                    let last_hop_rssi = r.i8()?;
                    let sender = r.u16()?;
                    let binding_index = r.u8()?;
                    let address_index = r.u8()?;
                    RxPacketInfo {
                        sender,
                        sender_ieee: None,
                        binding_index,
                        address_index,
                        last_hop_lqi,
                        last_hop_rssi,
                        last_hop_timestamp: 0,
                    }
                };
                Callback::IncomingMessage {
                    kind,
                    aps,
                    info,
                    payload: r.u8_prefixed()?.to_vec(),
                }
            }
            id::TRUST_CENTER_JOIN_HANDLER => Callback::TrustCenterJoin {
                node_id: r.u16()?,
                ieee: r.ieee()?,
                status: r.u8()?,
                decision: r.u8()?,
                parent: r.u16()?,
            },
            id::CHILD_JOIN_HANDLER => Callback::ChildJoin {
                index: r.u8()?,
                joining: r.bool()?,
                node_id: r.u16()?,
                ieee: r.ieee()?,
                node_type: r.u8()?,
            },
            id::INCOMING_ROUTE_RECORD_HANDLER => {
                let source = r.u16()?;
                let ieee = r.ieee()?;
                let last_hop_lqi = r.u8()?;
                let last_hop_rssi = r.i8()?;
                let count = usize::from(r.u8()?);
                Callback::IncomingRouteRecord {
                    source,
                    ieee,
                    last_hop_lqi,
                    last_hop_rssi,
                    relays: r.u16_list(count)?,
                }
            }
            id::ZIGBEE_KEY_ESTABLISHMENT_HANDLER => Callback::ZigbeeKeyEstablishment {
                partner: r.ieee()?,
                status: r.u8()?,
            },
            id::ENERGY_SCAN_RESULT_HANDLER => Callback::EnergyScanResult {
                channel: r.u8()?,
                max_rssi: r.i8()?,
            },
            id::SCAN_COMPLETE_HANDLER => Callback::ScanComplete {
                channel: r.u8()?,
                status: Status::read(&mut r, v)?,
            },
            id::ID_CONFLICT_HANDLER => Callback::IdConflict { node_id: r.u16()? },
            id::INCOMING_ROUTE_ERROR_HANDLER => Callback::IncomingRouteError {
                status: Status::read(&mut r, v)?,
                target: r.u16()?,
            },
            id::INCOMING_NETWORK_STATUS_HANDLER => Callback::IncomingNetworkStatus {
                code: r.u8()?,
                target: r.u16()?,
            },
            id::INCOMING_MANY_TO_ONE_ROUTE_REQUEST_HANDLER => {
                Callback::IncomingManyToOneRouteRequest {
                    source: r.u16()?,
                    ieee: r.ieee()?,
                    cost: r.u8()?,
                }
            }
            id::STACK_TOKEN_CHANGED_HANDLER => Callback::StackTokenChanged { token: r.u16()? },
            id::NO_CALLBACKS => Callback::NoCallbacks,
            other => Callback::Other {
                frame_id: other,
                params: params.to_vec(),
            },
        })
    }

    /// Encode the parameters (the NCP side).
    pub fn encode(&self, v: ProtocolVersion) -> Vec<u8> {
        let mut w = Vec::with_capacity(48);
        match self {
            Callback::StackStatus { status } => status.write(&mut w),
            Callback::MessageSent {
                status,
                kind,
                destination,
                aps,
                tag,
            } => {
                if v.sl_status() {
                    status.write(&mut w);
                    w.put_u8(*kind);
                    w.put_u16(*destination);
                    aps.write(&mut w);
                    w.put_u16(*tag);
                } else {
                    w.put_u8(*kind);
                    w.put_u16(*destination);
                    aps.write(&mut w);
                    w.put_u8(*tag as u8);
                    status.write(&mut w);
                }
                w.put_u8(0); // message contents: tag only (MESSAGE_TAG_ONLY_IN_CALLBACK)
            }
            Callback::IncomingMessage {
                kind,
                aps,
                info,
                payload,
            } => {
                w.put_u8(*kind);
                aps.write(&mut w);
                if v.sl_status() {
                    info.write(&mut w);
                } else {
                    w.put_u8(info.last_hop_lqi);
                    w.put_u8(info.last_hop_rssi as u8);
                    w.put_u16(info.sender);
                    w.put_u8(info.binding_index);
                    w.put_u8(info.address_index);
                }
                w.put_u8_prefixed(payload);
            }
            Callback::TrustCenterJoin {
                node_id,
                ieee,
                status,
                decision,
                parent,
            } => {
                w.put_u16(*node_id);
                w.put_ieee(*ieee);
                w.put_u8(*status);
                w.put_u8(*decision);
                w.put_u16(*parent);
            }
            Callback::ChildJoin {
                index,
                joining,
                node_id,
                ieee,
                node_type,
            } => {
                w.put_u8(*index);
                w.put_u8(u8::from(*joining));
                w.put_u16(*node_id);
                w.put_ieee(*ieee);
                w.put_u8(*node_type);
            }
            Callback::IncomingRouteRecord {
                source,
                ieee,
                last_hop_lqi,
                last_hop_rssi,
                relays,
            } => {
                w.put_u16(*source);
                w.put_ieee(*ieee);
                w.put_u8(*last_hop_lqi);
                w.put_u8(*last_hop_rssi as u8);
                w.put_u8(relays.len() as u8);
                relays.iter().for_each(|r| w.put_u16(*r));
            }
            Callback::ZigbeeKeyEstablishment { partner, status } => {
                w.put_ieee(*partner);
                w.put_u8(*status);
            }
            Callback::EnergyScanResult { channel, max_rssi } => {
                w.put_u8(*channel);
                w.put_u8(*max_rssi as u8);
            }
            Callback::ScanComplete { channel, status } => {
                w.put_u8(*channel);
                status.write(&mut w);
            }
            Callback::IdConflict { node_id } => w.put_u16(*node_id),
            Callback::IncomingRouteError { status, target } => {
                status.write(&mut w);
                w.put_u16(*target);
            }
            Callback::IncomingNetworkStatus { code, target } => {
                w.put_u8(*code);
                w.put_u16(*target);
            }
            Callback::IncomingManyToOneRouteRequest { source, ieee, cost } => {
                w.put_u16(*source);
                w.put_ieee(*ieee);
                w.put_u8(*cost);
            }
            Callback::StackTokenChanged { token } => w.put_u16(*token),
            Callback::NoCallbacks => {}
            Callback::Other { params, .. } => w.put_bytes(params),
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callbacks_round_trip_in_both_layouts() {
        for v in [ProtocolVersion(13), ProtocolVersion(17)] {
            let aps = ApsFrame {
                profile_id: 0x0104,
                cluster_id: 0x0402,
                source_endpoint: 1,
                destination_endpoint: 1,
                options: 0,
                group_id: 0,
                sequence: 4,
            };
            let info = RxPacketInfo {
                sender: 0x4A2B,
                sender_ieee: v.sl_status().then_some(Ieee(0x00124B0000000001)),
                binding_index: 0xFF,
                address_index: 0xFF,
                last_hop_lqi: 200,
                last_hop_rssi: -50,
                last_hop_timestamp: if v.sl_status() { 1234 } else { 0 },
            };
            let callbacks = vec![
                Callback::StackStatus {
                    status: Status::network_up(v),
                },
                Callback::MessageSent {
                    status: Status::ok(v),
                    kind: 0,
                    destination: 0x4A2B,
                    aps,
                    tag: 9,
                },
                Callback::IncomingMessage {
                    kind: 0,
                    aps,
                    info,
                    payload: vec![0x18, 1, 0x0A, 0, 0, 0x29, 0x66, 0x08],
                },
                Callback::TrustCenterJoin {
                    node_id: 0x4A2B,
                    ieee: Ieee(1),
                    status: 1,
                    decision: 0,
                    parent: 0,
                },
                Callback::ChildJoin {
                    index: 0,
                    joining: true,
                    node_id: 0x4A2B,
                    ieee: Ieee(1),
                    node_type: 4,
                },
                Callback::IncomingRouteRecord {
                    source: 0x4A2B,
                    ieee: Ieee(1),
                    last_hop_lqi: 99,
                    last_hop_rssi: -70,
                    relays: vec![0x1111, 0x2222],
                },
                Callback::ZigbeeKeyEstablishment {
                    partner: Ieee(1),
                    status: 0x06,
                },
                Callback::EnergyScanResult {
                    channel: 15,
                    max_rssi: -90,
                },
                Callback::ScanComplete {
                    channel: 26,
                    status: Status::ok(v),
                },
                Callback::IdConflict { node_id: 7 },
                Callback::IncomingRouteError {
                    status: Status::fail(v),
                    target: 7,
                },
                Callback::IncomingNetworkStatus {
                    code: 0x10,
                    target: 7,
                },
                Callback::IncomingManyToOneRouteRequest {
                    source: 7,
                    ieee: Ieee(2),
                    cost: 3,
                },
                Callback::StackTokenChanged { token: 0x0100 },
                Callback::NoCallbacks,
                Callback::Other {
                    frame_id: 0x0147,
                    params: vec![1, 2],
                },
            ];
            for c in callbacks {
                assert_eq!(
                    Callback::decode(v, c.frame_id(), &c.encode(v)),
                    Ok(c.clone()),
                    "{c:?}"
                );
            }
        }
    }
}
