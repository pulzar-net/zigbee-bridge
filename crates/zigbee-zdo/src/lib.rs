//! Zigbee Device Object (ZDO) frames, Zigbee specification section 2.4.
//!
//! [`Message`] is one ZDO frame: a transaction sequence number and a typed
//! [`Body`]. Requests and responses both parse and encode, so a host and a
//! simulated device share one definition. Clusters this crate does not
//! decode pass through as [`Body::Other`].
#![cfg_attr(
    not(test),
    deny(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )
)]

use zigbee_types::wire::{Reader, Truncated, Writer};
use zigbee_types::{ExtPanId, Ieee, NodeId};

/// ZDO cluster ids. A response's id is its request's with bit 15 set.
pub mod cluster {
    #![allow(missing_docs)]
    pub const NWK_ADDR_REQ: u16 = 0x0000;
    pub const IEEE_ADDR_REQ: u16 = 0x0001;
    pub const NODE_DESC_REQ: u16 = 0x0002;
    pub const POWER_DESC_REQ: u16 = 0x0003;
    pub const SIMPLE_DESC_REQ: u16 = 0x0004;
    pub const ACTIVE_EP_REQ: u16 = 0x0005;
    pub const MATCH_DESC_REQ: u16 = 0x0006;
    pub const DEVICE_ANNCE: u16 = 0x0013;
    pub const BIND_REQ: u16 = 0x0021;
    pub const UNBIND_REQ: u16 = 0x0022;
    pub const MGMT_LQI_REQ: u16 = 0x0031;
    pub const MGMT_RTG_REQ: u16 = 0x0032;
    pub const MGMT_BIND_REQ: u16 = 0x0033;
    pub const MGMT_LEAVE_REQ: u16 = 0x0034;
    pub const MGMT_PERMIT_JOINING_REQ: u16 = 0x0036;
    pub const MGMT_NWK_UPDATE_REQ: u16 = 0x0038;
    pub const NWK_ADDR_RSP: u16 = 0x8000;
    pub const IEEE_ADDR_RSP: u16 = 0x8001;
    pub const NODE_DESC_RSP: u16 = 0x8002;
    pub const POWER_DESC_RSP: u16 = 0x8003;
    pub const SIMPLE_DESC_RSP: u16 = 0x8004;
    pub const ACTIVE_EP_RSP: u16 = 0x8005;
    pub const MATCH_DESC_RSP: u16 = 0x8006;
    pub const BIND_RSP: u16 = 0x8021;
    pub const UNBIND_RSP: u16 = 0x8022;
    pub const MGMT_LQI_RSP: u16 = 0x8031;
    pub const MGMT_RTG_RSP: u16 = 0x8032;
    pub const MGMT_BIND_RSP: u16 = 0x8033;
    pub const MGMT_LEAVE_RSP: u16 = 0x8034;
    pub const MGMT_PERMIT_JOINING_RSP: u16 = 0x8036;
    pub const MGMT_NWK_UPDATE_NOTIFY: u16 = 0x8038;
}

/// ZDO status codes.
pub mod status {
    #![allow(missing_docs)]
    pub const SUCCESS: u8 = 0x00;
    pub const INV_REQUESTTYPE: u8 = 0x80;
    pub const DEVICE_NOT_FOUND: u8 = 0x81;
    pub const INVALID_EP: u8 = 0x82;
    pub const NOT_ACTIVE: u8 = 0x83;
    pub const NOT_SUPPORTED: u8 = 0x84;
    pub const TIMEOUT: u8 = 0x85;
    pub const NO_MATCH: u8 = 0x86;
    pub const NO_ENTRY: u8 = 0x88;
    pub const NO_DESCRIPTOR: u8 = 0x89;
    pub const INSUFFICIENT_SPACE: u8 = 0x8A;
    pub const NOT_PERMITTED: u8 = 0x8B;
    pub const TABLE_FULL: u8 = 0x8C;
    pub const NOT_AUTHORIZED: u8 = 0x8D;
}

/// The ZDO profile id.
pub const PROFILE_ID: u16 = 0x0000;
/// The ZDO endpoint.
pub const ENDPOINT: u8 = 0x00;

/// A ZDO frame did not decode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Too short.
    #[error(transparent)]
    Truncated(#[from] Truncated),
    /// Bytes left over.
    #[error("{0} trailing bytes")]
    Trailing(usize),
    /// A bind address mode other than group (1) or extended (3).
    #[error("bind address mode {0} is not 1 or 3")]
    AddressMode(u8),
}

/// The node descriptor (2.3.2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeDescriptor {
    /// Logical type (bits 0-2), complex/user descriptor flags (bits 3-4).
    pub byte0: u8,
    /// APS flags (bits 0-2) and frequency band (bits 3-7).
    pub byte1: u8,
    /// MAC capability flags.
    pub mac_capabilities: u8,
    /// Manufacturer code.
    pub manufacturer_code: u16,
    /// Maximum NSDU size.
    pub max_buffer_size: u8,
    /// Maximum incoming transfer size.
    pub max_incoming_transfer: u16,
    /// Server mask (bits 9-15: stack compliance revision).
    pub server_mask: u16,
    /// Maximum outgoing transfer size.
    pub max_outgoing_transfer: u16,
    /// Descriptor capability field.
    pub descriptor_capability: u8,
}

/// A device's logical type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum LogicalType {
    /// The coordinator.
    Coordinator,
    /// A router.
    Router,
    /// An end device.
    EndDevice,
    /// A reserved value.
    Reserved(u8),
}

impl NodeDescriptor {
    /// The device's logical type.
    pub fn logical_type(&self) -> LogicalType {
        match self.byte0 & 0x07 {
            0 => LogicalType::Coordinator,
            1 => LogicalType::Router,
            2 => LogicalType::EndDevice,
            other => LogicalType::Reserved(other),
        }
    }

    /// The receiver is on when idle (not a sleepy device).
    pub fn rx_on_when_idle(&self) -> bool {
        self.mac_capabilities & 0x08 != 0
    }

    /// Mains powered.
    pub fn mains_powered(&self) -> bool {
        self.mac_capabilities & 0x04 != 0
    }

    /// The stack compliance revision (21 and later for Zigbee 3.0
    /// devices; 0 for pre-R21 devices).
    pub fn stack_compliance_revision(&self) -> u8 {
        (self.server_mask >> 9) as u8
    }

    fn read(r: &mut Reader<'_>) -> Result<Self, Truncated> {
        Ok(NodeDescriptor {
            byte0: r.u8()?,
            byte1: r.u8()?,
            mac_capabilities: r.u8()?,
            manufacturer_code: r.u16()?,
            max_buffer_size: r.u8()?,
            max_incoming_transfer: r.u16()?,
            server_mask: r.u16()?,
            max_outgoing_transfer: r.u16()?,
            descriptor_capability: r.u8()?,
        })
    }

    fn write(&self, w: &mut Vec<u8>) {
        w.put_u8(self.byte0);
        w.put_u8(self.byte1);
        w.put_u8(self.mac_capabilities);
        w.put_u16(self.manufacturer_code);
        w.put_u8(self.max_buffer_size);
        w.put_u16(self.max_incoming_transfer);
        w.put_u16(self.server_mask);
        w.put_u16(self.max_outgoing_transfer);
        w.put_u8(self.descriptor_capability);
    }
}

/// A simple descriptor: one endpoint and its clusters (2.3.2.5).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SimpleDescriptor {
    /// Endpoint, 1 to 240.
    pub endpoint: u8,
    /// Application profile.
    pub profile_id: u16,
    /// Device id within the profile.
    pub device_id: u16,
    /// Device version (low 4 bits).
    pub device_version: u8,
    /// Server (input) clusters.
    pub input_clusters: Vec<u16>,
    /// Client (output) clusters.
    pub output_clusters: Vec<u16>,
}

impl SimpleDescriptor {
    fn read(r: &mut Reader<'_>) -> Result<Self, Truncated> {
        let endpoint = r.u8()?;
        let profile_id = r.u16()?;
        let device_id = r.u16()?;
        let device_version = r.u8()?;
        let inputs = usize::from(r.u8()?);
        let input_clusters = r.u16_list(inputs)?;
        let outputs = usize::from(r.u8()?);
        let output_clusters = r.u16_list(outputs)?;
        Ok(SimpleDescriptor {
            endpoint,
            profile_id,
            device_id,
            device_version,
            input_clusters,
            output_clusters,
        })
    }

    fn write(&self, w: &mut Vec<u8>) {
        w.put_u8(self.endpoint);
        w.put_u16(self.profile_id);
        w.put_u16(self.device_id);
        w.put_u8(self.device_version);
        w.put_u8(self.input_clusters.len().min(255) as u8);
        self.input_clusters
            .iter()
            .take(255)
            .for_each(|c| w.put_u16(*c));
        w.put_u8(self.output_clusters.len().min(255) as u8);
        self.output_clusters
            .iter()
            .take(255)
            .for_each(|c| w.put_u16(*c));
    }
}

/// A neighbor table entry, from `Mgmt_Lqi_rsp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Neighbor {
    /// The neighbor's extended PAN id.
    pub extended_pan_id: ExtPanId,
    /// Its IEEE address.
    pub ieee: Ieee,
    /// Its network address.
    pub nwk: NodeId,
    /// Device type (bits 0-1), rx on when idle (2-3), relationship (4-6).
    pub flags: u8,
    /// Permit joining (bits 0-1).
    pub permit_joining: u8,
    /// Tree depth.
    pub depth: u8,
    /// Link quality.
    pub lqi: u8,
}

impl Neighbor {
    /// 0 coordinator, 1 router, 2 end device, 3 unknown.
    pub fn device_type(&self) -> u8 {
        self.flags & 0x03
    }

    /// 0 parent, 1 child, 2 sibling, 3 none, 4 previous child.
    pub fn relationship(&self) -> u8 {
        (self.flags >> 4) & 0x07
    }
}

/// A routing table entry, from `Mgmt_Rtg_rsp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Route {
    /// Destination.
    pub destination: NodeId,
    /// Status (bits 0-2) and flags.
    pub flags: u8,
    /// Next hop.
    pub next_hop: NodeId,
}

/// Where a binding points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum BindTarget {
    /// A group.
    Group(u16),
    /// A device's endpoint.
    Endpoint {
        /// The device.
        ieee: Ieee,
        /// Its endpoint.
        endpoint: u8,
    },
}

impl BindTarget {
    fn read(r: &mut Reader<'_>) -> Result<Self, Error> {
        match r.u8()? {
            0x01 => Ok(BindTarget::Group(r.u16()?)),
            0x03 => Ok(BindTarget::Endpoint {
                ieee: r.ieee()?,
                endpoint: r.u8()?,
            }),
            other => Err(Error::AddressMode(other)),
        }
    }

    fn write(&self, w: &mut Vec<u8>) {
        match self {
            BindTarget::Group(g) => {
                w.put_u8(0x01);
                w.put_u16(*g);
            }
            BindTarget::Endpoint { ieee, endpoint } => {
                w.put_u8(0x03);
                w.put_ieee(*ieee);
                w.put_u8(*endpoint);
            }
        }
    }
}

/// A binding table entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Binding {
    /// Source device.
    pub source: Ieee,
    /// Source endpoint.
    pub source_endpoint: u8,
    /// Cluster.
    pub cluster: u16,
    /// Destination.
    pub target: BindTarget,
}

/// Address list in `NWK_addr_rsp` / `IEEE_addr_rsp` (extended requests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssociatedDevices {
    /// Index of the first address.
    pub start_index: u8,
    /// Network addresses of associated devices.
    pub addresses: Vec<NodeId>,
}

/// A ZDO frame body.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Body {
    NwkAddrReq {
        ieee: Ieee,
        request_type: u8,
        start_index: u8,
    },
    IeeeAddrReq {
        nwk: NodeId,
        request_type: u8,
        start_index: u8,
    },
    NodeDescReq {
        nwk: NodeId,
    },
    PowerDescReq {
        nwk: NodeId,
    },
    SimpleDescReq {
        nwk: NodeId,
        endpoint: u8,
    },
    ActiveEpReq {
        nwk: NodeId,
    },
    MatchDescReq {
        nwk: NodeId,
        profile_id: u16,
        input_clusters: Vec<u16>,
        output_clusters: Vec<u16>,
    },
    DeviceAnnce {
        nwk: NodeId,
        ieee: Ieee,
        capability: u8,
    },
    BindReq(Binding),
    UnbindReq(Binding),
    MgmtLqiReq {
        start_index: u8,
    },
    MgmtRtgReq {
        start_index: u8,
    },
    MgmtBindReq {
        start_index: u8,
    },
    MgmtLeaveReq {
        ieee: Ieee,
        flags: u8,
    },
    MgmtPermitJoiningReq {
        duration: u8,
        tc_significance: u8,
    },
    MgmtNwkUpdateReq {
        channel_mask: u32,
        scan_duration: u8,
        scan_count: Option<u8>,
        nwk_update_id: Option<u8>,
        manager: Option<NodeId>,
    },
    NwkAddrRsp {
        status: u8,
        ieee: Ieee,
        nwk: NodeId,
        associated: Option<AssociatedDevices>,
    },
    IeeeAddrRsp {
        status: u8,
        ieee: Ieee,
        nwk: NodeId,
        associated: Option<AssociatedDevices>,
    },
    NodeDescRsp {
        status: u8,
        nwk: NodeId,
        descriptor: Option<NodeDescriptor>,
    },
    PowerDescRsp {
        status: u8,
        nwk: NodeId,
        descriptor: Option<u16>,
    },
    SimpleDescRsp {
        status: u8,
        nwk: NodeId,
        descriptor: Option<SimpleDescriptor>,
    },
    ActiveEpRsp {
        status: u8,
        nwk: NodeId,
        endpoints: Vec<u8>,
    },
    MatchDescRsp {
        status: u8,
        nwk: NodeId,
        endpoints: Vec<u8>,
    },
    BindRsp {
        status: u8,
    },
    UnbindRsp {
        status: u8,
    },
    MgmtLqiRsp {
        status: u8,
        total: u8,
        start_index: u8,
        neighbors: Vec<Neighbor>,
    },
    MgmtRtgRsp {
        status: u8,
        total: u8,
        start_index: u8,
        routes: Vec<Route>,
    },
    MgmtBindRsp {
        status: u8,
        total: u8,
        start_index: u8,
        bindings: Vec<Binding>,
    },
    MgmtLeaveRsp {
        status: u8,
    },
    MgmtPermitJoiningRsp {
        status: u8,
    },
    MgmtNwkUpdateNotify {
        status: u8,
        scanned_channels: u32,
        total_transmissions: u16,
        transmission_failures: u16,
        energy: Vec<u8>,
    },
    /// A cluster this crate does not decode.
    Other {
        cluster: u16,
        payload: Vec<u8>,
    },
}

/// A ZDO frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Transaction sequence number, echoed in the response.
    pub tsn: u8,
    /// The body.
    pub body: Body,
}

fn read_binding(r: &mut Reader<'_>) -> Result<Binding, Error> {
    Ok(Binding {
        source: r.ieee()?,
        source_endpoint: r.u8()?,
        cluster: r.u16()?,
        target: BindTarget::read(r)?,
    })
}

fn write_binding(b: &Binding, w: &mut Vec<u8>) {
    w.put_ieee(b.source);
    w.put_u8(b.source_endpoint);
    w.put_u16(b.cluster);
    b.target.write(w);
}

fn read_list8(r: &mut Reader<'_>) -> Result<Vec<u8>, Truncated> {
    let n = usize::from(r.u8()?);
    Ok(r.bytes(n)?.to_vec())
}

fn read_associated(r: &mut Reader<'_>) -> Result<Option<AssociatedDevices>, Truncated> {
    if r.is_empty() {
        return Ok(None);
    }
    let count = usize::from(r.u8()?);
    let start_index = r.u8()?;
    Ok(Some(AssociatedDevices {
        start_index,
        addresses: r.u16_list(count)?,
    }))
}

fn write_associated(a: &Option<AssociatedDevices>, w: &mut Vec<u8>) {
    if let Some(a) = a {
        w.put_u8(a.addresses.len().min(255) as u8);
        w.put_u8(a.start_index);
        a.addresses.iter().take(255).for_each(|n| w.put_u16(*n));
    }
}

/// Read a table header and `count` entries, bounding the allocation by
/// the input.
fn read_table<T>(
    r: &mut Reader<'_>,
    entry_size: usize,
    mut read: impl FnMut(&mut Reader<'_>) -> Result<T, Error>,
) -> Result<(u8, u8, u8, Vec<T>), Error> {
    let status = r.u8()?;
    if r.is_empty() {
        return Ok((status, 0, 0, Vec::new()));
    }
    let total = r.u8()?;
    let start = r.u8()?;
    let count = usize::from(r.u8()?);
    let mut entries = Vec::with_capacity(count.min(r.remaining() / entry_size.max(1)));
    for _ in 0..count {
        entries.push(read(r)?);
    }
    Ok((status, total, start, entries))
}

impl Message {
    /// The cluster id for this body.
    pub fn cluster(&self) -> u16 {
        use crate::cluster as c;
        match &self.body {
            Body::NwkAddrReq { .. } => c::NWK_ADDR_REQ,
            Body::IeeeAddrReq { .. } => c::IEEE_ADDR_REQ,
            Body::NodeDescReq { .. } => c::NODE_DESC_REQ,
            Body::PowerDescReq { .. } => c::POWER_DESC_REQ,
            Body::SimpleDescReq { .. } => c::SIMPLE_DESC_REQ,
            Body::ActiveEpReq { .. } => c::ACTIVE_EP_REQ,
            Body::MatchDescReq { .. } => c::MATCH_DESC_REQ,
            Body::DeviceAnnce { .. } => c::DEVICE_ANNCE,
            Body::BindReq(_) => c::BIND_REQ,
            Body::UnbindReq(_) => c::UNBIND_REQ,
            Body::MgmtLqiReq { .. } => c::MGMT_LQI_REQ,
            Body::MgmtRtgReq { .. } => c::MGMT_RTG_REQ,
            Body::MgmtBindReq { .. } => c::MGMT_BIND_REQ,
            Body::MgmtLeaveReq { .. } => c::MGMT_LEAVE_REQ,
            Body::MgmtPermitJoiningReq { .. } => c::MGMT_PERMIT_JOINING_REQ,
            Body::MgmtNwkUpdateReq { .. } => c::MGMT_NWK_UPDATE_REQ,
            Body::NwkAddrRsp { .. } => c::NWK_ADDR_RSP,
            Body::IeeeAddrRsp { .. } => c::IEEE_ADDR_RSP,
            Body::NodeDescRsp { .. } => c::NODE_DESC_RSP,
            Body::PowerDescRsp { .. } => c::POWER_DESC_RSP,
            Body::SimpleDescRsp { .. } => c::SIMPLE_DESC_RSP,
            Body::ActiveEpRsp { .. } => c::ACTIVE_EP_RSP,
            Body::MatchDescRsp { .. } => c::MATCH_DESC_RSP,
            Body::BindRsp { .. } => c::BIND_RSP,
            Body::UnbindRsp { .. } => c::UNBIND_RSP,
            Body::MgmtLqiRsp { .. } => c::MGMT_LQI_RSP,
            Body::MgmtRtgRsp { .. } => c::MGMT_RTG_RSP,
            Body::MgmtBindRsp { .. } => c::MGMT_BIND_RSP,
            Body::MgmtLeaveRsp { .. } => c::MGMT_LEAVE_RSP,
            Body::MgmtPermitJoiningRsp { .. } => c::MGMT_PERMIT_JOINING_RSP,
            Body::MgmtNwkUpdateNotify { .. } => c::MGMT_NWK_UPDATE_NOTIFY,
            Body::Other { cluster, .. } => *cluster,
        }
    }

    /// The status of a response body.
    pub fn status(&self) -> Option<u8> {
        match &self.body {
            Body::NwkAddrRsp { status, .. }
            | Body::IeeeAddrRsp { status, .. }
            | Body::NodeDescRsp { status, .. }
            | Body::PowerDescRsp { status, .. }
            | Body::SimpleDescRsp { status, .. }
            | Body::ActiveEpRsp { status, .. }
            | Body::MatchDescRsp { status, .. }
            | Body::BindRsp { status }
            | Body::UnbindRsp { status }
            | Body::MgmtLqiRsp { status, .. }
            | Body::MgmtRtgRsp { status, .. }
            | Body::MgmtBindRsp { status, .. }
            | Body::MgmtLeaveRsp { status }
            | Body::MgmtPermitJoiningRsp { status }
            | Body::MgmtNwkUpdateNotify { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Parse a ZDO frame of `cluster` (tsn first).
    pub fn parse(cluster: u16, data: &[u8]) -> Result<Message, Error> {
        use crate::cluster as c;
        let mut r = Reader::new(data);
        let tsn = r.u8()?;
        let body = match cluster {
            c::NWK_ADDR_REQ => Body::NwkAddrReq {
                ieee: r.ieee()?,
                request_type: r.u8()?,
                start_index: r.u8()?,
            },
            c::IEEE_ADDR_REQ => Body::IeeeAddrReq {
                nwk: r.u16()?,
                request_type: r.u8()?,
                start_index: r.u8()?,
            },
            c::NODE_DESC_REQ => Body::NodeDescReq { nwk: r.u16()? },
            c::POWER_DESC_REQ => Body::PowerDescReq { nwk: r.u16()? },
            c::SIMPLE_DESC_REQ => Body::SimpleDescReq {
                nwk: r.u16()?,
                endpoint: r.u8()?,
            },
            c::ACTIVE_EP_REQ => Body::ActiveEpReq { nwk: r.u16()? },
            c::MATCH_DESC_REQ => {
                let nwk = r.u16()?;
                let profile_id = r.u16()?;
                let n = usize::from(r.u8()?);
                let input_clusters = r.u16_list(n)?;
                let n = usize::from(r.u8()?);
                Body::MatchDescReq {
                    nwk,
                    profile_id,
                    input_clusters,
                    output_clusters: r.u16_list(n)?,
                }
            }
            c::DEVICE_ANNCE => Body::DeviceAnnce {
                nwk: r.u16()?,
                ieee: r.ieee()?,
                capability: r.u8()?,
            },
            c::BIND_REQ => Body::BindReq(read_binding(&mut r)?),
            c::UNBIND_REQ => Body::UnbindReq(read_binding(&mut r)?),
            c::MGMT_LQI_REQ => Body::MgmtLqiReq {
                start_index: r.u8()?,
            },
            c::MGMT_RTG_REQ => Body::MgmtRtgReq {
                start_index: r.u8()?,
            },
            c::MGMT_BIND_REQ => Body::MgmtBindReq {
                start_index: r.u8()?,
            },
            c::MGMT_LEAVE_REQ => Body::MgmtLeaveReq {
                ieee: r.ieee()?,
                flags: r.u8()?,
            },
            c::MGMT_PERMIT_JOINING_REQ => Body::MgmtPermitJoiningReq {
                duration: r.u8()?,
                tc_significance: r.u8()?,
            },
            c::MGMT_NWK_UPDATE_REQ => {
                let channel_mask = r.u32()?;
                let scan_duration = r.u8()?;
                let scan_count = if scan_duration <= 5 {
                    Some(r.u8()?)
                } else {
                    None
                };
                let nwk_update_id = if scan_duration >= 0xFE {
                    Some(r.u8()?)
                } else {
                    None
                };
                let manager = if scan_duration == 0xFF {
                    Some(r.u16()?)
                } else {
                    None
                };
                Body::MgmtNwkUpdateReq {
                    channel_mask,
                    scan_duration,
                    scan_count,
                    nwk_update_id,
                    manager,
                }
            }
            c::NWK_ADDR_RSP | c::IEEE_ADDR_RSP => {
                let status = r.u8()?;
                let ieee = r.ieee()?;
                let nwk = r.u16()?;
                let associated = read_associated(&mut r)?;
                if cluster == c::NWK_ADDR_RSP {
                    Body::NwkAddrRsp {
                        status,
                        ieee,
                        nwk,
                        associated,
                    }
                } else {
                    Body::IeeeAddrRsp {
                        status,
                        ieee,
                        nwk,
                        associated,
                    }
                }
            }
            c::NODE_DESC_RSP => {
                let status = r.u8()?;
                let nwk = r.u16()?;
                let descriptor = if r.is_empty() {
                    None
                } else {
                    Some(NodeDescriptor::read(&mut r)?)
                };
                Body::NodeDescRsp {
                    status,
                    nwk,
                    descriptor,
                }
            }
            c::POWER_DESC_RSP => {
                let status = r.u8()?;
                let nwk = r.u16()?;
                let descriptor = if r.is_empty() { None } else { Some(r.u16()?) };
                Body::PowerDescRsp {
                    status,
                    nwk,
                    descriptor,
                }
            }
            c::SIMPLE_DESC_RSP => {
                let status = r.u8()?;
                let nwk = r.u16()?;
                let len = if r.is_empty() {
                    0
                } else {
                    usize::from(r.u8()?)
                };
                let descriptor = if len == 0 {
                    None
                } else {
                    let mut inner = Reader::new(r.bytes(len)?);
                    let d = SimpleDescriptor::read(&mut inner)?;
                    if !inner.is_empty() {
                        return Err(Error::Trailing(inner.remaining()));
                    }
                    Some(d)
                };
                Body::SimpleDescRsp {
                    status,
                    nwk,
                    descriptor,
                }
            }
            c::ACTIVE_EP_RSP | c::MATCH_DESC_RSP => {
                let status = r.u8()?;
                let nwk = r.u16()?;
                let endpoints = if r.is_empty() {
                    Vec::new()
                } else {
                    read_list8(&mut r)?
                };
                if cluster == c::ACTIVE_EP_RSP {
                    Body::ActiveEpRsp {
                        status,
                        nwk,
                        endpoints,
                    }
                } else {
                    Body::MatchDescRsp {
                        status,
                        nwk,
                        endpoints,
                    }
                }
            }
            c::BIND_RSP => Body::BindRsp { status: r.u8()? },
            c::UNBIND_RSP => Body::UnbindRsp { status: r.u8()? },
            c::MGMT_LEAVE_RSP => Body::MgmtLeaveRsp { status: r.u8()? },
            c::MGMT_PERMIT_JOINING_RSP => Body::MgmtPermitJoiningRsp { status: r.u8()? },
            c::MGMT_LQI_RSP => {
                let (status, total, start_index, neighbors) = read_table(&mut r, 22, |r| {
                    Ok(Neighbor {
                        extended_pan_id: r.ext_pan_id()?,
                        ieee: r.ieee()?,
                        nwk: r.u16()?,
                        flags: r.u8()?,
                        permit_joining: r.u8()?,
                        depth: r.u8()?,
                        lqi: r.u8()?,
                    })
                })?;
                Body::MgmtLqiRsp {
                    status,
                    total,
                    start_index,
                    neighbors,
                }
            }
            c::MGMT_RTG_RSP => {
                let (status, total, start_index, routes) = read_table(&mut r, 5, |r| {
                    Ok(Route {
                        destination: r.u16()?,
                        flags: r.u8()?,
                        next_hop: r.u16()?,
                    })
                })?;
                Body::MgmtRtgRsp {
                    status,
                    total,
                    start_index,
                    routes,
                }
            }
            c::MGMT_BIND_RSP => {
                let (status, total, start_index, bindings) = read_table(&mut r, 14, read_binding)?;
                Body::MgmtBindRsp {
                    status,
                    total,
                    start_index,
                    bindings,
                }
            }
            c::MGMT_NWK_UPDATE_NOTIFY => Body::MgmtNwkUpdateNotify {
                status: r.u8()?,
                scanned_channels: r.u32()?,
                total_transmissions: r.u16()?,
                transmission_failures: r.u16()?,
                energy: read_list8(&mut r)?,
            },
            other => Body::Other {
                cluster: other,
                payload: r.rest().to_vec(),
            },
        };
        match r.remaining() {
            0 => Ok(Message { tsn, body }),
            n => Err(Error::Trailing(n)),
        }
    }

    /// Encode: the cluster id and the frame (tsn first).
    pub fn encode(&self) -> (u16, Vec<u8>) {
        let mut w = Vec::with_capacity(16);
        w.put_u8(self.tsn);
        match &self.body {
            Body::NwkAddrReq {
                ieee,
                request_type,
                start_index,
            } => {
                w.put_ieee(*ieee);
                w.put_u8(*request_type);
                w.put_u8(*start_index);
            }
            Body::IeeeAddrReq {
                nwk,
                request_type,
                start_index,
            } => {
                w.put_u16(*nwk);
                w.put_u8(*request_type);
                w.put_u8(*start_index);
            }
            Body::NodeDescReq { nwk } | Body::PowerDescReq { nwk } | Body::ActiveEpReq { nwk } => {
                w.put_u16(*nwk)
            }
            Body::SimpleDescReq { nwk, endpoint } => {
                w.put_u16(*nwk);
                w.put_u8(*endpoint);
            }
            Body::MatchDescReq {
                nwk,
                profile_id,
                input_clusters,
                output_clusters,
            } => {
                w.put_u16(*nwk);
                w.put_u16(*profile_id);
                for list in [input_clusters, output_clusters] {
                    w.put_u8(list.len().min(255) as u8);
                    list.iter().take(255).for_each(|c| w.put_u16(*c));
                }
            }
            Body::DeviceAnnce {
                nwk,
                ieee,
                capability,
            } => {
                w.put_u16(*nwk);
                w.put_ieee(*ieee);
                w.put_u8(*capability);
            }
            Body::BindReq(b) | Body::UnbindReq(b) => write_binding(b, &mut w),
            Body::MgmtLqiReq { start_index }
            | Body::MgmtRtgReq { start_index }
            | Body::MgmtBindReq { start_index } => w.put_u8(*start_index),
            Body::MgmtLeaveReq { ieee, flags } => {
                w.put_ieee(*ieee);
                w.put_u8(*flags);
            }
            Body::MgmtPermitJoiningReq {
                duration,
                tc_significance,
            } => {
                w.put_u8(*duration);
                w.put_u8(*tc_significance);
            }
            Body::MgmtNwkUpdateReq {
                channel_mask,
                scan_duration,
                scan_count,
                nwk_update_id,
                manager,
            } => {
                w.put_u32(*channel_mask);
                w.put_u8(*scan_duration);
                if *scan_duration <= 5 {
                    w.put_u8(scan_count.unwrap_or(1));
                }
                if *scan_duration >= 0xFE {
                    w.put_u8(nwk_update_id.unwrap_or(0));
                }
                if *scan_duration == 0xFF {
                    w.put_u16(manager.unwrap_or(0));
                }
            }
            Body::NwkAddrRsp {
                status,
                ieee,
                nwk,
                associated,
            }
            | Body::IeeeAddrRsp {
                status,
                ieee,
                nwk,
                associated,
            } => {
                w.put_u8(*status);
                w.put_ieee(*ieee);
                w.put_u16(*nwk);
                write_associated(associated, &mut w);
            }
            Body::NodeDescRsp {
                status,
                nwk,
                descriptor,
            } => {
                w.put_u8(*status);
                w.put_u16(*nwk);
                if let Some(d) = descriptor {
                    d.write(&mut w);
                }
            }
            Body::PowerDescRsp {
                status,
                nwk,
                descriptor,
            } => {
                w.put_u8(*status);
                w.put_u16(*nwk);
                if let Some(d) = descriptor {
                    w.put_u16(*d);
                }
            }
            Body::SimpleDescRsp {
                status,
                nwk,
                descriptor,
            } => {
                w.put_u8(*status);
                w.put_u16(*nwk);
                match descriptor {
                    Some(d) => {
                        let mut inner = Vec::new();
                        d.write(&mut inner);
                        w.put_u8_prefixed(&inner);
                    }
                    None => w.put_u8(0),
                }
            }
            Body::ActiveEpRsp {
                status,
                nwk,
                endpoints,
            }
            | Body::MatchDescRsp {
                status,
                nwk,
                endpoints,
            } => {
                w.put_u8(*status);
                w.put_u16(*nwk);
                w.put_u8_prefixed(endpoints);
            }
            Body::BindRsp { status }
            | Body::UnbindRsp { status }
            | Body::MgmtLeaveRsp { status }
            | Body::MgmtPermitJoiningRsp { status } => w.put_u8(*status),
            Body::MgmtLqiRsp {
                status,
                total,
                start_index,
                neighbors,
            } => {
                w.put_u8(*status);
                w.put_u8(*total);
                w.put_u8(*start_index);
                w.put_u8(neighbors.len().min(255) as u8);
                for n in neighbors.iter().take(255) {
                    w.put_ext_pan_id(n.extended_pan_id);
                    w.put_ieee(n.ieee);
                    w.put_u16(n.nwk);
                    w.put_u8(n.flags);
                    w.put_u8(n.permit_joining);
                    w.put_u8(n.depth);
                    w.put_u8(n.lqi);
                }
            }
            Body::MgmtRtgRsp {
                status,
                total,
                start_index,
                routes,
            } => {
                w.put_u8(*status);
                w.put_u8(*total);
                w.put_u8(*start_index);
                w.put_u8(routes.len().min(255) as u8);
                for route in routes.iter().take(255) {
                    w.put_u16(route.destination);
                    w.put_u8(route.flags);
                    w.put_u16(route.next_hop);
                }
            }
            Body::MgmtBindRsp {
                status,
                total,
                start_index,
                bindings,
            } => {
                w.put_u8(*status);
                w.put_u8(*total);
                w.put_u8(*start_index);
                w.put_u8(bindings.len().min(255) as u8);
                bindings
                    .iter()
                    .take(255)
                    .for_each(|b| write_binding(b, &mut w));
            }
            Body::MgmtNwkUpdateNotify {
                status,
                scanned_channels,
                total_transmissions,
                transmission_failures,
                energy,
            } => {
                w.put_u8(*status);
                w.put_u32(*scanned_channels);
                w.put_u16(*total_transmissions);
                w.put_u16(*transmission_failures);
                w.put_u8_prefixed(energy);
            }
            Body::Other { payload, .. } => w.put_bytes(payload),
        }
        (self.cluster(), w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(body: Body) {
        let m = Message { tsn: 42, body };
        let (cluster, bytes) = m.encode();
        assert_eq!(Message::parse(cluster, &bytes), Ok(m));
    }

    #[test]
    fn every_body_round_trips() {
        let ieee = Ieee(0x00124B0001020304);
        let nd = NodeDescriptor {
            byte0: 0x02,
            byte1: 0x40,
            mac_capabilities: 0x80,
            manufacturer_code: 0x115F,
            max_buffer_size: 127,
            max_incoming_transfer: 100,
            server_mask: 0x2C00,
            max_outgoing_transfer: 100,
            descriptor_capability: 0,
        };
        let sd = SimpleDescriptor {
            endpoint: 1,
            profile_id: 0x0104,
            device_id: 0x0302,
            device_version: 1,
            input_clusters: vec![0, 1, 0x0402],
            output_clusters: vec![0x0019],
        };
        let binding = Binding {
            source: ieee,
            source_endpoint: 1,
            cluster: 0x0402,
            target: BindTarget::Endpoint {
                ieee: Ieee(1),
                endpoint: 1,
            },
        };
        for body in [
            Body::NwkAddrReq {
                ieee,
                request_type: 0,
                start_index: 0,
            },
            Body::IeeeAddrReq {
                nwk: 0x1234,
                request_type: 1,
                start_index: 0,
            },
            Body::NodeDescReq { nwk: 0x1234 },
            Body::SimpleDescReq {
                nwk: 0x1234,
                endpoint: 1,
            },
            Body::ActiveEpReq { nwk: 0x1234 },
            Body::MatchDescReq {
                nwk: 0xFFFD,
                profile_id: 0x0104,
                input_clusters: vec![0x0019],
                output_clusters: vec![],
            },
            Body::DeviceAnnce {
                nwk: 0x1234,
                ieee,
                capability: 0x80,
            },
            Body::BindReq(binding),
            Body::UnbindReq(Binding {
                target: BindTarget::Group(5),
                ..binding
            }),
            Body::MgmtLqiReq { start_index: 3 },
            Body::MgmtLeaveReq { ieee, flags: 0x40 },
            Body::MgmtPermitJoiningReq {
                duration: 180,
                tc_significance: 1,
            },
            Body::MgmtNwkUpdateReq {
                channel_mask: 1 << 15,
                scan_duration: 0xFE,
                scan_count: None,
                nwk_update_id: Some(3),
                manager: None,
            },
            Body::MgmtNwkUpdateReq {
                channel_mask: 0x07FFF800,
                scan_duration: 3,
                scan_count: Some(1),
                nwk_update_id: None,
                manager: None,
            },
            Body::NwkAddrRsp {
                status: 0,
                ieee,
                nwk: 0x1234,
                associated: None,
            },
            Body::IeeeAddrRsp {
                status: 0,
                ieee,
                nwk: 0x1234,
                associated: Some(AssociatedDevices {
                    start_index: 0,
                    addresses: vec![1, 2],
                }),
            },
            Body::NodeDescRsp {
                status: 0,
                nwk: 0x1234,
                descriptor: Some(nd),
            },
            Body::NodeDescRsp {
                status: status::DEVICE_NOT_FOUND,
                nwk: 0x1234,
                descriptor: None,
            },
            Body::SimpleDescRsp {
                status: 0,
                nwk: 0x1234,
                descriptor: Some(sd),
            },
            Body::SimpleDescRsp {
                status: status::INVALID_EP,
                nwk: 0x1234,
                descriptor: None,
            },
            Body::ActiveEpRsp {
                status: 0,
                nwk: 0x1234,
                endpoints: vec![1, 2],
            },
            Body::BindRsp { status: 0 },
            Body::MgmtLqiRsp {
                status: 0,
                total: 1,
                start_index: 0,
                neighbors: vec![Neighbor {
                    extended_pan_id: ExtPanId(7),
                    ieee,
                    nwk: 0x1234,
                    flags: 0x25,
                    permit_joining: 2,
                    depth: 1,
                    lqi: 180,
                }],
            },
            Body::MgmtRtgRsp {
                status: 0,
                total: 1,
                start_index: 0,
                routes: vec![Route {
                    destination: 0x1234,
                    flags: 0,
                    next_hop: 0x5678,
                }],
            },
            Body::MgmtBindRsp {
                status: 0,
                total: 1,
                start_index: 0,
                bindings: vec![binding],
            },
            Body::MgmtLeaveRsp { status: 0 },
            Body::MgmtPermitJoiningRsp { status: 0 },
            Body::MgmtNwkUpdateNotify {
                status: 0,
                scanned_channels: 0x07FFF800,
                total_transmissions: 10,
                transmission_failures: 1,
                energy: vec![10, 20],
            },
            Body::Other {
                cluster: 0x0040,
                payload: vec![1, 2],
            },
        ] {
            round_trip(body);
        }
        assert_eq!(nd.logical_type(), LogicalType::EndDevice);
        assert_eq!(nd.stack_compliance_revision(), 22);
        assert!(!nd.rx_on_when_idle());
    }

    #[test]
    fn rejects_truncated_and_hostile_tables() {
        assert!(Message::parse(cluster::NODE_DESC_REQ, &[1]).is_err());
        // Claims 255 neighbors with no bytes for them.
        assert!(Message::parse(cluster::MGMT_LQI_RSP, &[1, 0, 255, 0, 255]).is_err());
        assert!(Message::parse(
            cluster::BIND_REQ,
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0x02]
        )
        .is_err());
    }
}
