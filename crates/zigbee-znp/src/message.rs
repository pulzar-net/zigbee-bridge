//! Typed requests the host sends and indications the NCP sends.
//!
//! Every type here encodes to a [`Frame`] and decodes from one, so the
//! client, the simulator and the fuzz targets share one definition of each
//! layout.

use thiserror::Error;
use zigbee_primitives::wire::{Reader, Truncated, Writer};
use zigbee_primitives::{Ieee, Key, NodeId};

use crate::frame::{Frame, Kind};
use crate::ids::{af, app_cnf, subsystem as sub, sys, util, zdo};

/// A frame did not decode.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    /// Too short.
    #[error(transparent)]
    Truncated(#[from] Truncated),
    /// Not a frame this type knows.
    #[error("unknown MT command {kind:?} {subsystem:#04x} {id:#04x}")]
    Unknown {
        /// Type.
        kind: Kind,
        /// Subsystem.
        subsystem: u8,
        /// Command.
        id: u8,
    },
    /// A length field disagrees with the data.
    #[error("length field {0} does not fit the frame")]
    Length(usize),
}

/// A request from the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `SYS_RESET_REQ` (asynchronous; the NCP answers with a reset
    /// indication).
    SysReset {
        /// Soft (1) or hard (0) reset.
        soft: bool,
    },
    /// `SYS_PING`.
    SysPing,
    /// `SYS_VERSION`.
    SysVersion,
    /// `SYS_OSAL_NV_ITEM_INIT`.
    OsalNvItemInit {
        /// Item.
        id: u16,
        /// Item length.
        len: u16,
        /// Initial value.
        init: Vec<u8>,
    },
    /// `SYS_OSAL_NV_READ`.
    OsalNvRead {
        /// Item.
        id: u16,
        /// Offset.
        offset: u8,
    },
    /// `SYS_OSAL_NV_WRITE`.
    OsalNvWrite {
        /// Item.
        id: u16,
        /// Offset.
        offset: u8,
        /// Value.
        value: Vec<u8>,
    },
    /// `SYS_OSAL_NV_DELETE`.
    OsalNvDelete {
        /// Item.
        id: u16,
        /// Its length.
        len: u16,
    },
    /// `SYS_OSAL_NV_LENGTH`.
    OsalNvLength {
        /// Item.
        id: u16,
    },
    /// `SYS_SET_TX_POWER`.
    SetTxPower {
        /// dBm.
        dbm: i8,
    },
    /// `SYS_NV_LENGTH` (extended NV).
    NvLength {
        /// System id.
        sysid: u8,
        /// Item.
        item: u16,
        /// Sub-item.
        sub: u16,
    },
    /// `SYS_NV_READ` (extended NV).
    NvRead {
        /// System id.
        sysid: u8,
        /// Item.
        item: u16,
        /// Sub-item.
        sub: u16,
        /// Offset.
        offset: u16,
        /// Bytes to read.
        len: u8,
    },
    /// `SYS_NV_WRITE` (extended NV).
    NvWrite {
        /// System id.
        sysid: u8,
        /// Item.
        item: u16,
        /// Sub-item.
        sub: u16,
        /// Offset.
        offset: u16,
        /// Value.
        value: Vec<u8>,
    },
    /// `UTIL_GET_DEVICE_INFO`.
    UtilGetDeviceInfo,
    /// `ZDO_SET_LINK_KEY`.
    ZdoSetLinkKey {
        /// The device's network address.
        nwk: NodeId,
        /// Its IEEE address.
        ieee: Ieee,
        /// The key.
        key: Key,
    },
    /// `ZDO_NWK_DISCOVERY_REQ`: an active scan.
    ZdoNwkDiscovery {
        /// Channels.
        channel_mask: u32,
        /// Scan duration exponent per channel.
        scan_duration: u8,
    },
    /// `ZDO_SEND_DATA`: a raw ZDO frame with the host's sequence number.
    ZdoSendData {
        /// Destination.
        destination: NodeId,
        /// ZDO sequence number.
        tsn: u8,
        /// ZDO cluster.
        cluster: u16,
        /// The ZDO payload after the sequence number.
        data: Vec<u8>,
    },
    /// `ZDO_MGMT_PERMIT_JOIN_REQ`.
    ZdoMgmtPermitJoin {
        /// Address mode ([`crate::ids::addr_mode`]).
        addr_mode: u8,
        /// Destination.
        destination: NodeId,
        /// Seconds; 0 closes.
        duration: u8,
        /// Trust center significance.
        tc_significance: u8,
    },
    /// `ZDO_MGMT_NWK_UPDATE_REQ`.
    ZdoMgmtNwkUpdate {
        /// Destination.
        destination: NodeId,
        /// Address mode.
        addr_mode: u8,
        /// Channels.
        channel_mask: u32,
        /// Scan duration, or 0xFE to change channel.
        scan_duration: u8,
        /// Scans per channel.
        scan_count: u8,
        /// Network manager.
        manager: NodeId,
    },
    /// `ZDO_MSG_CB_REGISTER`.
    ZdoMsgCbRegister {
        /// Cluster, or 0xFFFF for every ZDO cluster.
        cluster: u16,
    },
    /// `ZDO_STARTUP_FROM_APP`.
    ZdoStartupFromApp {
        /// Delay, milliseconds.
        delay: u16,
    },
    /// `ZDO_EXT_NWK_INFO`.
    ZdoExtNwkInfo,
    /// `AF_REGISTER`.
    AfRegister {
        /// Endpoint.
        endpoint: u8,
        /// Profile.
        profile: u16,
        /// Device id.
        device_id: u16,
        /// Device version.
        version: u8,
        /// Server clusters.
        input_clusters: Vec<u16>,
        /// Client clusters.
        output_clusters: Vec<u16>,
    },
    /// `AF_DATA_REQUEST`.
    AfDataRequest {
        /// Destination.
        destination: NodeId,
        /// Destination endpoint.
        destination_endpoint: u8,
        /// Source endpoint.
        source_endpoint: u8,
        /// Cluster.
        cluster: u16,
        /// Transaction id, named again in the confirm.
        trans_id: u8,
        /// Options ([`crate::ids::af_option`]).
        options: u8,
        /// Radius.
        radius: u8,
        /// Payload.
        data: Vec<u8>,
    },
    /// `AF_DATA_REQUEST_EXT`.
    AfDataRequestExt {
        /// Address mode.
        addr_mode: u8,
        /// Destination: a group, network or IEEE address.
        destination: u64,
        /// Destination endpoint.
        destination_endpoint: u8,
        /// Destination PAN (0 for this one).
        destination_pan: u16,
        /// Source endpoint.
        source_endpoint: u8,
        /// Cluster.
        cluster: u16,
        /// Transaction id.
        trans_id: u8,
        /// Options.
        options: u8,
        /// Radius.
        radius: u8,
        /// Payload.
        data: Vec<u8>,
    },
    /// `APP_CNF_SET_ALLOWREJOIN_TC_POLICY`.
    SetAllowRejoinTcPolicy {
        /// Allow.
        allow: bool,
    },
    /// `APP_CNF_BDB_ADD_INSTALLCODE`.
    BdbAddInstallCode {
        /// 1: install code with CRC; 2: derived key.
        format: u8,
        /// The device.
        ieee: Ieee,
        /// The code or key.
        code: Vec<u8>,
    },
    /// `APP_CNF_BDB_SET_JOINUSESINSTALLCODEKEY`.
    BdbSetJoinUsesInstallCodeKey {
        /// Only devices with an install code may join.
        required: bool,
    },
    /// `APP_CNF_BDB_START_COMMISSIONING`.
    BdbStartCommissioning {
        /// Mode ([`crate::ids::commissioning`]).
        mode: u8,
    },
    /// `APP_CNF_BDB_SET_CHANNEL`.
    BdbSetChannel {
        /// The primary (or secondary) channel set.
        primary: bool,
        /// Channels.
        channel_mask: u32,
    },
    /// `APP_CNF_BDB_SET_TC_REQUIRE_KEY_EXCHANGE`.
    BdbSetTcRequireKeyExchange {
        /// Required.
        required: bool,
    },
}

fn u8_list(r: &mut Reader<'_>) -> Result<Vec<u16>, DecodeError> {
    let n = r.u8()?;
    Ok(r.u16_list(usize::from(n))?)
}

fn put_u16_list(w: &mut Vec<u8>, list: &[u16]) {
    let list = list.get(..list.len().min(255)).unwrap_or(list);
    w.put_u8(list.len() as u8);
    for c in list {
        w.put_u16(*c);
    }
}

impl Request {
    /// The frame type: everything but a reset is synchronous.
    pub fn kind(&self) -> Kind {
        match self {
            Request::SysReset { .. } => Kind::Areq,
            _ => Kind::Sreq,
        }
    }

    /// Subsystem and command id.
    pub fn command(&self) -> (u8, u8) {
        use Request::*;
        match self {
            SysReset { .. } => (sub::SYS, sys::RESET_REQ),
            SysPing => (sub::SYS, sys::PING),
            SysVersion => (sub::SYS, sys::VERSION),
            OsalNvItemInit { .. } => (sub::SYS, sys::OSAL_NV_ITEM_INIT),
            OsalNvRead { .. } => (sub::SYS, sys::OSAL_NV_READ),
            OsalNvWrite { .. } => (sub::SYS, sys::OSAL_NV_WRITE),
            OsalNvDelete { .. } => (sub::SYS, sys::OSAL_NV_DELETE),
            OsalNvLength { .. } => (sub::SYS, sys::OSAL_NV_LENGTH),
            SetTxPower { .. } => (sub::SYS, sys::SET_TX_POWER),
            NvLength { .. } => (sub::SYS, sys::NV_LENGTH),
            NvRead { .. } => (sub::SYS, sys::NV_READ),
            NvWrite { .. } => (sub::SYS, sys::NV_WRITE),
            UtilGetDeviceInfo => (sub::UTIL, util::GET_DEVICE_INFO),
            ZdoSetLinkKey { .. } => (sub::ZDO, zdo::SET_LINK_KEY),
            ZdoNwkDiscovery { .. } => (sub::ZDO, zdo::NWK_DISCOVERY_REQ),
            ZdoSendData { .. } => (sub::ZDO, zdo::SEND_DATA),
            ZdoMgmtPermitJoin { .. } => (sub::ZDO, zdo::MGMT_PERMIT_JOIN_REQ),
            ZdoMgmtNwkUpdate { .. } => (sub::ZDO, zdo::MGMT_NWK_UPDATE_REQ),
            ZdoMsgCbRegister { .. } => (sub::ZDO, zdo::MSG_CB_REGISTER),
            ZdoStartupFromApp { .. } => (sub::ZDO, zdo::STARTUP_FROM_APP),
            ZdoExtNwkInfo => (sub::ZDO, zdo::EXT_NWK_INFO),
            AfRegister { .. } => (sub::AF, af::REGISTER),
            AfDataRequest { .. } => (sub::AF, af::DATA_REQUEST),
            AfDataRequestExt { .. } => (sub::AF, af::DATA_REQUEST_EXT),
            SetAllowRejoinTcPolicy { .. } => (sub::APP_CNF, app_cnf::SET_ALLOWREJOIN_TC_POLICY),
            BdbAddInstallCode { .. } => (sub::APP_CNF, app_cnf::BDB_ADD_INSTALLCODE),
            BdbSetJoinUsesInstallCodeKey { .. } => {
                (sub::APP_CNF, app_cnf::BDB_SET_JOINUSESINSTALLCODEKEY)
            }
            BdbStartCommissioning { .. } => (sub::APP_CNF, app_cnf::BDB_START_COMMISSIONING),
            BdbSetChannel { .. } => (sub::APP_CNF, app_cnf::BDB_SET_CHANNEL),
            BdbSetTcRequireKeyExchange { .. } => {
                (sub::APP_CNF, app_cnf::BDB_SET_TC_REQUIRE_KEY_EXCHANGE)
            }
        }
    }

    /// The frame; [`Frame::encode`] refuses one over 250 bytes.
    pub fn encode(&self) -> Frame {
        use Request::*;
        let mut w = Vec::new();
        match self {
            SysReset { soft } => w.put_u8(u8::from(*soft)),
            SysPing | SysVersion | UtilGetDeviceInfo | ZdoExtNwkInfo => {}
            OsalNvItemInit { id, len, init } => {
                w.put_u16(*id);
                w.put_u16(*len);
                w.put_u8_prefixed(init);
            }
            OsalNvRead { id, offset } => {
                w.put_u16(*id);
                w.put_u8(*offset);
            }
            OsalNvWrite { id, offset, value } => {
                w.put_u16(*id);
                w.put_u8(*offset);
                w.put_u8_prefixed(value);
            }
            OsalNvDelete { id, len } => {
                w.put_u16(*id);
                w.put_u16(*len);
            }
            OsalNvLength { id } => w.put_u16(*id),
            SetTxPower { dbm } => w.put_u8(*dbm as u8),
            NvLength { sysid, item, sub } => {
                w.put_u8(*sysid);
                w.put_u16(*item);
                w.put_u16(*sub);
            }
            NvRead {
                sysid,
                item,
                sub,
                offset,
                len,
            } => {
                w.put_u8(*sysid);
                w.put_u16(*item);
                w.put_u16(*sub);
                w.put_u16(*offset);
                w.put_u8(*len);
            }
            NvWrite {
                sysid,
                item,
                sub,
                offset,
                value,
            } => {
                w.put_u8(*sysid);
                w.put_u16(*item);
                w.put_u16(*sub);
                w.put_u16(*offset);
                w.put_u8_prefixed(value);
            }
            ZdoSetLinkKey { nwk, ieee, key } => {
                w.put_u16(*nwk);
                w.put_ieee(*ieee);
                w.put_key(key);
            }
            ZdoNwkDiscovery {
                channel_mask,
                scan_duration,
            } => {
                w.put_u32(*channel_mask);
                w.put_u8(*scan_duration);
            }
            ZdoSendData {
                destination,
                tsn,
                cluster,
                data,
            } => {
                w.put_u16(*destination);
                w.put_u8(*tsn);
                w.put_u16(*cluster);
                w.put_u8_prefixed(data);
            }
            ZdoMgmtPermitJoin {
                addr_mode,
                destination,
                duration,
                tc_significance,
            } => {
                w.put_u8(*addr_mode);
                w.put_u16(*destination);
                w.put_u8(*duration);
                w.put_u8(*tc_significance);
            }
            ZdoMgmtNwkUpdate {
                destination,
                addr_mode,
                channel_mask,
                scan_duration,
                scan_count,
                manager,
            } => {
                w.put_u16(*destination);
                w.put_u8(*addr_mode);
                w.put_u32(*channel_mask);
                w.put_u8(*scan_duration);
                w.put_u8(*scan_count);
                w.put_u16(*manager);
            }
            ZdoMsgCbRegister { cluster } => w.put_u16(*cluster),
            ZdoStartupFromApp { delay } => w.put_u16(*delay),
            AfRegister {
                endpoint,
                profile,
                device_id,
                version,
                input_clusters,
                output_clusters,
            } => {
                w.put_u8(*endpoint);
                w.put_u16(*profile);
                w.put_u16(*device_id);
                w.put_u8(*version);
                w.put_u8(0); // latency: none
                put_u16_list(&mut w, input_clusters);
                put_u16_list(&mut w, output_clusters);
            }
            AfDataRequest {
                destination,
                destination_endpoint,
                source_endpoint,
                cluster,
                trans_id,
                options,
                radius,
                data,
            } => {
                w.put_u16(*destination);
                w.put_u8(*destination_endpoint);
                w.put_u8(*source_endpoint);
                w.put_u16(*cluster);
                w.put_u8(*trans_id);
                w.put_u8(*options);
                w.put_u8(*radius);
                w.put_u8_prefixed(data);
            }
            AfDataRequestExt {
                addr_mode,
                destination,
                destination_endpoint,
                destination_pan,
                source_endpoint,
                cluster,
                trans_id,
                options,
                radius,
                data,
            } => {
                w.put_u8(*addr_mode);
                w.put_u64(*destination);
                w.put_u8(*destination_endpoint);
                w.put_u16(*destination_pan);
                w.put_u8(*source_endpoint);
                w.put_u16(*cluster);
                w.put_u8(*trans_id);
                w.put_u8(*options);
                w.put_u8(*radius);
                w.put_u16(data.len() as u16);
                w.put_bytes(data);
            }
            SetAllowRejoinTcPolicy { allow } => w.put_u8(u8::from(*allow)),
            BdbAddInstallCode { format, ieee, code } => {
                w.put_u8(*format);
                w.put_ieee(*ieee);
                w.put_bytes(code);
            }
            BdbStartCommissioning { mode } => w.put_u8(*mode),
            BdbSetChannel {
                primary,
                channel_mask,
            } => {
                w.put_u8(u8::from(*primary));
                w.put_u32(*channel_mask);
            }
            BdbSetTcRequireKeyExchange { required } | BdbSetJoinUsesInstallCodeKey { required } => {
                w.put_u8(u8::from(*required))
            }
        }
        let (subsystem, id) = self.command();
        Frame::new(self.kind(), subsystem, id, w)
    }

    /// Decode a request (the simulator's side).
    pub fn decode(frame: &Frame) -> Result<Request, DecodeError> {
        use Request::*;
        let mut r = Reader::new(&frame.data);
        let unknown = || DecodeError::Unknown {
            kind: frame.kind,
            subsystem: frame.subsystem,
            id: frame.id,
        };
        let request = match (frame.kind, frame.subsystem, frame.id) {
            (Kind::Areq, sub::SYS, sys::RESET_REQ) => SysReset { soft: r.u8()? != 0 },
            (Kind::Sreq, sub::SYS, sys::PING) => SysPing,
            (Kind::Sreq, sub::SYS, sys::VERSION) => SysVersion,
            (Kind::Sreq, sub::SYS, sys::OSAL_NV_ITEM_INIT) => OsalNvItemInit {
                id: r.u16()?,
                len: r.u16()?,
                init: r.u8_prefixed()?.to_vec(),
            },
            (Kind::Sreq, sub::SYS, sys::OSAL_NV_READ) => OsalNvRead {
                id: r.u16()?,
                offset: r.u8()?,
            },
            (Kind::Sreq, sub::SYS, sys::OSAL_NV_WRITE) => OsalNvWrite {
                id: r.u16()?,
                offset: r.u8()?,
                value: r.u8_prefixed()?.to_vec(),
            },
            (Kind::Sreq, sub::SYS, sys::OSAL_NV_DELETE) => OsalNvDelete {
                id: r.u16()?,
                len: r.u16()?,
            },
            (Kind::Sreq, sub::SYS, sys::OSAL_NV_LENGTH) => OsalNvLength { id: r.u16()? },
            (Kind::Sreq, sub::SYS, sys::SET_TX_POWER) => SetTxPower { dbm: r.i8()? },
            (Kind::Sreq, sub::SYS, sys::NV_LENGTH) => NvLength {
                sysid: r.u8()?,
                item: r.u16()?,
                sub: r.u16()?,
            },
            (Kind::Sreq, sub::SYS, sys::NV_READ) => NvRead {
                sysid: r.u8()?,
                item: r.u16()?,
                sub: r.u16()?,
                offset: r.u16()?,
                len: r.u8()?,
            },
            (Kind::Sreq, sub::SYS, sys::NV_WRITE) => NvWrite {
                sysid: r.u8()?,
                item: r.u16()?,
                sub: r.u16()?,
                offset: r.u16()?,
                value: r.u8_prefixed()?.to_vec(),
            },
            (Kind::Sreq, sub::UTIL, util::GET_DEVICE_INFO) => UtilGetDeviceInfo,
            (Kind::Sreq, sub::ZDO, zdo::SET_LINK_KEY) => ZdoSetLinkKey {
                nwk: r.u16()?,
                ieee: r.ieee()?,
                key: r.key()?,
            },
            (Kind::Sreq, sub::ZDO, zdo::NWK_DISCOVERY_REQ) => ZdoNwkDiscovery {
                channel_mask: r.u32()?,
                scan_duration: r.u8()?,
            },
            (Kind::Sreq, sub::ZDO, zdo::SEND_DATA) => ZdoSendData {
                destination: r.u16()?,
                tsn: r.u8()?,
                cluster: r.u16()?,
                data: r.u8_prefixed()?.to_vec(),
            },
            (Kind::Sreq, sub::ZDO, zdo::MGMT_PERMIT_JOIN_REQ) => ZdoMgmtPermitJoin {
                addr_mode: r.u8()?,
                destination: r.u16()?,
                duration: r.u8()?,
                tc_significance: r.u8()?,
            },
            (Kind::Sreq, sub::ZDO, zdo::MGMT_NWK_UPDATE_REQ) => ZdoMgmtNwkUpdate {
                destination: r.u16()?,
                addr_mode: r.u8()?,
                channel_mask: r.u32()?,
                scan_duration: r.u8()?,
                scan_count: r.u8()?,
                manager: r.u16()?,
            },
            (Kind::Sreq, sub::ZDO, zdo::MSG_CB_REGISTER) => ZdoMsgCbRegister { cluster: r.u16()? },
            (Kind::Sreq, sub::ZDO, zdo::STARTUP_FROM_APP) => ZdoStartupFromApp { delay: r.u16()? },
            (Kind::Sreq, sub::ZDO, zdo::EXT_NWK_INFO) => ZdoExtNwkInfo,
            (Kind::Sreq, sub::AF, af::REGISTER) => {
                let endpoint = r.u8()?;
                let profile = r.u16()?;
                let device_id = r.u16()?;
                let version = r.u8()?;
                let _latency = r.u8()?;
                AfRegister {
                    endpoint,
                    profile,
                    device_id,
                    version,
                    input_clusters: u8_list(&mut r)?,
                    output_clusters: u8_list(&mut r)?,
                }
            }
            (Kind::Sreq, sub::AF, af::DATA_REQUEST) => AfDataRequest {
                destination: r.u16()?,
                destination_endpoint: r.u8()?,
                source_endpoint: r.u8()?,
                cluster: r.u16()?,
                trans_id: r.u8()?,
                options: r.u8()?,
                radius: r.u8()?,
                data: r.u8_prefixed()?.to_vec(),
            },
            (Kind::Sreq, sub::AF, af::DATA_REQUEST_EXT) => AfDataRequestExt {
                addr_mode: r.u8()?,
                destination: r.u64()?,
                destination_endpoint: r.u8()?,
                destination_pan: r.u16()?,
                source_endpoint: r.u8()?,
                cluster: r.u16()?,
                trans_id: r.u8()?,
                options: r.u8()?,
                radius: r.u8()?,
                data: {
                    let n = usize::from(r.u16()?);
                    r.bytes(n)?.to_vec()
                },
            },
            (Kind::Sreq, sub::APP_CNF, app_cnf::SET_ALLOWREJOIN_TC_POLICY) => {
                SetAllowRejoinTcPolicy {
                    allow: r.u8()? != 0,
                }
            }
            (Kind::Sreq, sub::APP_CNF, app_cnf::BDB_ADD_INSTALLCODE) => BdbAddInstallCode {
                format: r.u8()?,
                ieee: r.ieee()?,
                code: r.rest().to_vec(),
            },
            (Kind::Sreq, sub::APP_CNF, app_cnf::BDB_SET_JOINUSESINSTALLCODEKEY) => {
                BdbSetJoinUsesInstallCodeKey {
                    required: r.u8()? != 0,
                }
            }
            (Kind::Sreq, sub::APP_CNF, app_cnf::BDB_START_COMMISSIONING) => {
                BdbStartCommissioning { mode: r.u8()? }
            }
            (Kind::Sreq, sub::APP_CNF, app_cnf::BDB_SET_CHANNEL) => BdbSetChannel {
                primary: r.u8()? != 0,
                channel_mask: r.u32()?,
            },
            (Kind::Sreq, sub::APP_CNF, app_cnf::BDB_SET_TC_REQUIRE_KEY_EXCHANGE) => {
                BdbSetTcRequireKeyExchange {
                    required: r.u8()? != 0,
                }
            }
            _ => return Err(unknown()),
        };
        Ok(request)
    }
}

/// An `AF_INCOMING_MSG` or `AF_INCOMING_MSG_EXT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMsg {
    /// The group, 0 for none.
    pub group: u16,
    /// Cluster.
    pub cluster: u16,
    /// The sender's network address (the short form's only address).
    pub source: NodeId,
    /// The sender's IEEE address, when the extended form carried one.
    pub source_ieee: Option<Ieee>,
    /// Source endpoint.
    pub source_endpoint: u8,
    /// Destination endpoint.
    pub destination_endpoint: u8,
    /// Sent as a broadcast.
    pub broadcast: bool,
    /// Link quality.
    pub lqi: u8,
    /// Was APS-secured.
    pub secured: bool,
    /// The NCP's timestamp.
    pub timestamp: u32,
    /// APS counter.
    pub tsn: u8,
    /// Payload.
    pub data: Vec<u8>,
    /// The last hop's network address, when the firmware reports it.
    pub last_hop: Option<NodeId>,
    /// The extended form (long payloads, IEEE source).
    pub extended: bool,
}

/// A beacon from `ZDO_BEACON_NOTIFY_IND`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beacon {
    /// The sender.
    pub source: NodeId,
    /// PAN id.
    pub pan_id: u16,
    /// Channel.
    pub channel: u8,
    /// Accepting joins.
    pub permit_joining: bool,
    /// Router capacity left.
    pub router_capacity: bool,
    /// End device capacity left.
    pub device_capacity: bool,
    /// Protocol version.
    pub protocol_version: u8,
    /// Stack profile.
    pub stack_profile: u8,
    /// Link quality.
    pub lqi: u8,
    /// Depth.
    pub depth: u8,
    /// Network update id.
    pub update_id: u8,
    /// Extended PAN id.
    pub extended_pan_id: zigbee_primitives::ExtPanId,
}

/// An asynchronous frame from the NCP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Indication {
    /// `SYS_RESET_IND`.
    Reset {
        /// Why: 0 power-up, 1 external, 2 watchdog.
        reason: u8,
        /// Transport revision.
        transport: u8,
        /// Product ([`crate::ids::product`]).
        product: u8,
        /// Major release.
        major: u8,
        /// Minor release.
        minor: u8,
        /// Hardware revision.
        hw: u8,
    },
    /// `AF_DATA_CONFIRM`.
    DataConfirm {
        /// Status.
        status: u8,
        /// Endpoint.
        endpoint: u8,
        /// The request's transaction id.
        trans_id: u8,
    },
    /// `AF_INCOMING_MSG` or `AF_INCOMING_MSG_EXT`.
    Incoming(Box<IncomingMsg>),
    /// `ZDO_STATE_CHANGE_IND`.
    StateChange {
        /// The new state ([`crate::ids::state`]).
        state: u8,
    },
    /// `ZDO_END_DEVICE_ANNCE_IND`.
    DeviceAnnounce {
        /// Sender.
        source: NodeId,
        /// The device's network address.
        nwk: NodeId,
        /// Its IEEE address.
        ieee: Ieee,
        /// Capabilities.
        capabilities: u8,
    },
    /// `ZDO_SRC_RTG_IND`.
    SourceRoute {
        /// The originator.
        source: NodeId,
        /// Relays.
        relays: Vec<NodeId>,
    },
    /// `ZDO_BEACON_NOTIFY_IND`.
    Beacons(Vec<Beacon>),
    /// `ZDO_NWK_DISCOVERY_CNF`.
    DiscoveryDone {
        /// Status.
        status: u8,
    },
    /// `ZDO_LEAVE_IND`.
    Leave {
        /// Network address.
        nwk: NodeId,
        /// IEEE address.
        ieee: Ieee,
        /// A leave request rather than an indication.
        request: bool,
        /// Children leave too.
        remove_children: bool,
        /// The device will rejoin.
        rejoin: bool,
    },
    /// `ZDO_TC_DEV_IND`.
    TcDevice {
        /// Network address.
        nwk: NodeId,
        /// IEEE address.
        ieee: Ieee,
        /// Parent.
        parent: NodeId,
    },
    /// `ZDO_PERMIT_JOIN_IND`.
    PermitJoin {
        /// Seconds.
        duration: u8,
    },
    /// `ZDO_MSG_CB_INCOMING`: a raw ZDO frame.
    ZdoMessage {
        /// Sender.
        source: NodeId,
        /// Sent as a broadcast.
        broadcast: bool,
        /// Cluster.
        cluster: u16,
        /// APS-secured.
        secured: bool,
        /// ZDO sequence number.
        tsn: u8,
        /// MAC destination.
        mac_destination: NodeId,
        /// The payload after the sequence number.
        data: Vec<u8>,
    },
    /// `APP_CNF_BDB_COMMISSIONING_NOTIFICATION`.
    Commissioning {
        /// Status: 0 success, 1 in progress, others failures.
        status: u8,
        /// The mode that reported.
        mode: u8,
        /// Modes still to run.
        remaining: u8,
    },
}

impl Indication {
    /// Decode an asynchronous frame.
    pub fn decode(frame: &Frame) -> Result<Indication, DecodeError> {
        use Indication::*;
        let mut r = Reader::new(&frame.data);
        if frame.kind != Kind::Areq {
            return Err(DecodeError::Unknown {
                kind: frame.kind,
                subsystem: frame.subsystem,
                id: frame.id,
            });
        }
        let ind = match (frame.subsystem, frame.id) {
            (sub::SYS, sys::RESET_IND) => Reset {
                reason: r.u8()?,
                transport: r.u8()?,
                product: r.u8()?,
                major: r.u8()?,
                minor: r.u8()?,
                hw: r.u8()?,
            },
            (sub::AF, af::DATA_CONFIRM) => DataConfirm {
                status: r.u8()?,
                endpoint: r.u8()?,
                trans_id: r.u8()?,
            },
            (sub::AF, af::INCOMING_MSG) => {
                let group = r.u16()?;
                let cluster = r.u16()?;
                let source = r.u16()?;
                let source_endpoint = r.u8()?;
                let destination_endpoint = r.u8()?;
                let broadcast = r.u8()? != 0;
                let lqi = r.u8()?;
                let secured = r.u8()? != 0;
                let timestamp = r.u32()?;
                let tsn = r.u8()?;
                let data = r.u8_prefixed()?.to_vec();
                let last_hop = if r.remaining() >= 2 {
                    Some(r.u16()?)
                } else {
                    None
                };
                Incoming(Box::new(IncomingMsg {
                    group,
                    cluster,
                    source,
                    source_ieee: None,
                    source_endpoint,
                    destination_endpoint,
                    broadcast,
                    lqi,
                    secured,
                    timestamp,
                    tsn,
                    data,
                    last_hop,
                    extended: false,
                }))
            }
            (sub::AF, af::INCOMING_MSG_EXT) => {
                let group = r.u16()?;
                let cluster = r.u16()?;
                let mode = r.u8()?;
                let address = r.u64()?;
                let (source, source_ieee) = if mode == crate::ids::addr_mode::IEEE {
                    (0xFFFE, Some(Ieee(address)))
                } else {
                    (address as u16, None)
                };
                let source_endpoint = r.u8()?;
                let _source_pan = r.u16()?;
                let destination_endpoint = r.u8()?;
                let broadcast = r.u8()? != 0;
                let lqi = r.u8()?;
                let secured = r.u8()? != 0;
                let timestamp = r.u32()?;
                let tsn = r.u8()?;
                let n = usize::from(r.u16()?);
                let data = r.bytes(n)?.to_vec();
                Incoming(Box::new(IncomingMsg {
                    group,
                    cluster,
                    source,
                    source_ieee,
                    source_endpoint,
                    destination_endpoint,
                    broadcast,
                    lqi,
                    secured,
                    timestamp,
                    tsn,
                    data,
                    last_hop: None,
                    extended: true,
                }))
            }
            (sub::ZDO, zdo::STATE_CHANGE_IND) => StateChange { state: r.u8()? },
            (sub::ZDO, zdo::END_DEVICE_ANNCE_IND) => DeviceAnnounce {
                source: r.u16()?,
                nwk: r.u16()?,
                ieee: r.ieee()?,
                capabilities: r.u8()?,
            },
            (sub::ZDO, zdo::SRC_RTG_IND) => {
                let source = r.u16()?;
                let n = r.u8()?;
                SourceRoute {
                    source,
                    relays: r.u16_list(usize::from(n))?,
                }
            }
            (sub::ZDO, zdo::BEACON_NOTIFY_IND) => {
                let n = usize::from(r.u8()?);
                let mut beacons = Vec::with_capacity(n.min(r.remaining() / 21 + 1));
                for _ in 0..n {
                    beacons.push(Beacon {
                        source: r.u16()?,
                        pan_id: r.u16()?,
                        channel: r.u8()?,
                        permit_joining: r.u8()? != 0,
                        router_capacity: r.u8()? != 0,
                        device_capacity: r.u8()? != 0,
                        protocol_version: r.u8()?,
                        stack_profile: r.u8()?,
                        lqi: r.u8()?,
                        depth: r.u8()?,
                        update_id: r.u8()?,
                        extended_pan_id: r.ext_pan_id()?,
                    });
                }
                Beacons(beacons)
            }
            (sub::ZDO, zdo::NWK_DISCOVERY_CNF) => DiscoveryDone { status: r.u8()? },
            (sub::ZDO, zdo::LEAVE_IND) => Leave {
                nwk: r.u16()?,
                ieee: r.ieee()?,
                request: r.u8()? != 0,
                remove_children: r.u8()? != 0,
                rejoin: r.u8()? != 0,
            },
            (sub::ZDO, zdo::TC_DEV_IND) => TcDevice {
                nwk: r.u16()?,
                ieee: r.ieee()?,
                parent: r.u16()?,
            },
            (sub::ZDO, zdo::PERMIT_JOIN_IND) => PermitJoin { duration: r.u8()? },
            (sub::ZDO, zdo::MSG_CB_INCOMING) => ZdoMessage {
                source: r.u16()?,
                broadcast: r.u8()? != 0,
                cluster: r.u16()?,
                secured: r.u8()? != 0,
                tsn: r.u8()?,
                mac_destination: r.u16()?,
                data: r.rest().to_vec(),
            },
            (sub::APP_CNF, app_cnf::BDB_COMMISSIONING_NOTIFICATION) => Commissioning {
                status: r.u8()?,
                mode: r.u8()?,
                remaining: r.u8()?,
            },
            _ => {
                return Err(DecodeError::Unknown {
                    kind: frame.kind,
                    subsystem: frame.subsystem,
                    id: frame.id,
                })
            }
        };
        Ok(ind)
    }

    /// Encode (the simulator's side).
    pub fn encode(&self) -> Frame {
        use Indication::*;
        let mut w = Vec::new();
        let (subsystem, id) = match self {
            Reset {
                reason,
                transport,
                product,
                major,
                minor,
                hw,
            } => {
                w.put_bytes(&[*reason, *transport, *product, *major, *minor, *hw]);
                (sub::SYS, sys::RESET_IND)
            }
            DataConfirm {
                status,
                endpoint,
                trans_id,
            } => {
                w.put_bytes(&[*status, *endpoint, *trans_id]);
                (sub::AF, af::DATA_CONFIRM)
            }
            Incoming(m) if m.extended => {
                w.put_u16(m.group);
                w.put_u16(m.cluster);
                match m.source_ieee {
                    Some(ieee) => {
                        w.put_u8(crate::ids::addr_mode::IEEE);
                        w.put_ieee(ieee);
                    }
                    None => {
                        w.put_u8(crate::ids::addr_mode::SHORT);
                        w.put_u64(u64::from(m.source));
                    }
                }
                w.put_u8(m.source_endpoint);
                w.put_u16(0);
                w.put_u8(m.destination_endpoint);
                w.put_u8(u8::from(m.broadcast));
                w.put_u8(m.lqi);
                w.put_u8(u8::from(m.secured));
                w.put_u32(m.timestamp);
                w.put_u8(m.tsn);
                w.put_u16(m.data.len() as u16);
                w.put_bytes(&m.data);
                (sub::AF, af::INCOMING_MSG_EXT)
            }
            Incoming(m) => {
                w.put_u16(m.group);
                w.put_u16(m.cluster);
                w.put_u16(m.source);
                w.put_u8(m.source_endpoint);
                w.put_u8(m.destination_endpoint);
                w.put_u8(u8::from(m.broadcast));
                w.put_u8(m.lqi);
                w.put_u8(u8::from(m.secured));
                w.put_u32(m.timestamp);
                w.put_u8(m.tsn);
                w.put_u8_prefixed(&m.data);
                if let Some(hop) = m.last_hop {
                    w.put_u16(hop);
                    w.put_u8(0); // radius
                }
                (sub::AF, af::INCOMING_MSG)
            }
            StateChange { state } => {
                w.put_u8(*state);
                (sub::ZDO, zdo::STATE_CHANGE_IND)
            }
            DeviceAnnounce {
                source,
                nwk,
                ieee,
                capabilities,
            } => {
                w.put_u16(*source);
                w.put_u16(*nwk);
                w.put_ieee(*ieee);
                w.put_u8(*capabilities);
                (sub::ZDO, zdo::END_DEVICE_ANNCE_IND)
            }
            SourceRoute { source, relays } => {
                w.put_u16(*source);
                put_u16_list(&mut w, relays);
                (sub::ZDO, zdo::SRC_RTG_IND)
            }
            Beacons(beacons) => {
                w.put_u8(beacons.len().min(255) as u8);
                for b in beacons.iter().take(255) {
                    w.put_u16(b.source);
                    w.put_u16(b.pan_id);
                    w.put_u8(b.channel);
                    w.put_u8(u8::from(b.permit_joining));
                    w.put_u8(u8::from(b.router_capacity));
                    w.put_u8(u8::from(b.device_capacity));
                    w.put_u8(b.protocol_version);
                    w.put_u8(b.stack_profile);
                    w.put_u8(b.lqi);
                    w.put_u8(b.depth);
                    w.put_u8(b.update_id);
                    w.put_ext_pan_id(b.extended_pan_id);
                }
                (sub::ZDO, zdo::BEACON_NOTIFY_IND)
            }
            DiscoveryDone { status } => {
                w.put_u8(*status);
                (sub::ZDO, zdo::NWK_DISCOVERY_CNF)
            }
            Leave {
                nwk,
                ieee,
                request,
                remove_children,
                rejoin,
            } => {
                w.put_u16(*nwk);
                w.put_ieee(*ieee);
                w.put_u8(u8::from(*request));
                w.put_u8(u8::from(*remove_children));
                w.put_u8(u8::from(*rejoin));
                (sub::ZDO, zdo::LEAVE_IND)
            }
            TcDevice { nwk, ieee, parent } => {
                w.put_u16(*nwk);
                w.put_ieee(*ieee);
                w.put_u16(*parent);
                (sub::ZDO, zdo::TC_DEV_IND)
            }
            PermitJoin { duration } => {
                w.put_u8(*duration);
                (sub::ZDO, zdo::PERMIT_JOIN_IND)
            }
            ZdoMessage {
                source,
                broadcast,
                cluster,
                secured,
                tsn,
                mac_destination,
                data,
            } => {
                w.put_u16(*source);
                w.put_u8(u8::from(*broadcast));
                w.put_u16(*cluster);
                w.put_u8(u8::from(*secured));
                w.put_u8(*tsn);
                w.put_u16(*mac_destination);
                w.put_bytes(data);
                (sub::ZDO, zdo::MSG_CB_INCOMING)
            }
            Commissioning {
                status,
                mode,
                remaining,
            } => {
                w.put_bytes(&[*status, *mode, *remaining]);
                (sub::APP_CNF, app_cnf::BDB_COMMISSIONING_NOTIFICATION)
            }
        };
        Frame::new(Kind::Areq, subsystem, id, w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        let requests = [
            Request::SysReset { soft: true },
            Request::OsalNvWrite {
                id: 0x0062,
                offset: 0,
                value: vec![1; 16],
            },
            Request::NvRead {
                sysid: 1,
                item: 7,
                sub: 0,
                offset: 0,
                len: 12,
            },
            Request::ZdoSendData {
                destination: 0x1234,
                tsn: 9,
                cluster: 0x0005,
                data: vec![0x34, 0x12],
            },
            Request::AfRegister {
                endpoint: 1,
                profile: 0x0104,
                device_id: 5,
                version: 0,
                input_clusters: vec![0, 6],
                output_clusters: vec![0x0019],
            },
            Request::AfDataRequestExt {
                addr_mode: 1,
                destination: 0x0010,
                destination_endpoint: 0xFF,
                destination_pan: 0,
                source_endpoint: 1,
                cluster: 6,
                trans_id: 3,
                options: 0,
                radius: 30,
                data: vec![1, 2, 3],
            },
            Request::BdbAddInstallCode {
                format: 2,
                ieee: Ieee(1),
                code: vec![7; 16],
            },
        ];
        for r in requests {
            let f = r.encode();
            assert_eq!(Request::decode(&f), Ok(r));
            assert!(f.encode().is_ok());
        }
    }

    #[test]
    fn indications_round_trip() {
        let incoming = IncomingMsg {
            group: 0,
            cluster: 0x0402,
            source: 0x1234,
            source_ieee: None,
            source_endpoint: 1,
            destination_endpoint: 1,
            broadcast: false,
            lqi: 200,
            secured: false,
            timestamp: 5,
            tsn: 1,
            data: vec![0x18, 1, 0x0A],
            last_hop: Some(0x1234),
            extended: false,
        };
        let inds = [
            Indication::Incoming(Box::new(incoming.clone())),
            Indication::Incoming(Box::new(IncomingMsg {
                extended: true,
                last_hop: None,
                source_ieee: Some(Ieee(9)),
                source: 0xFFFE,
                ..incoming
            })),
            Indication::SourceRoute {
                source: 1,
                relays: vec![2, 3],
            },
            Indication::ZdoMessage {
                source: 1,
                broadcast: false,
                cluster: 0x8005,
                secured: false,
                tsn: 4,
                mac_destination: 0,
                data: vec![0, 1, 0, 1, 1],
            },
        ];
        for i in inds {
            assert_eq!(Indication::decode(&i.encode()), Ok(i));
        }
    }
}
