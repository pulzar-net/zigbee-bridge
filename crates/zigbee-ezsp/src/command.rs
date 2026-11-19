//! Typed EZSP commands (host to NCP) and their responses (NCP to host).
//!
//! Both directions encode and decode, so the host and the repository's NCP
//! simulator share one definition of every frame. Layouts that changed
//! between protocol version 13 and 14 take the version.

use zigbee_types::wire::{Reader, Truncated, Writer};
use zigbee_types::{Ieee, Key, NodeId};

use crate::ids::frame as id;
use crate::types::{
    ApsFrame, ApsKeyMetadata, InitialSecurityState, NetworkKeyInfo, NetworkParameters,
    ProtocolVersion, SecManContext, Status,
};

/// A frame's parameters did not decode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// Too short.
    #[error(transparent)]
    Truncated(#[from] Truncated),
    /// Bytes were left over.
    #[error("{0} trailing bytes")]
    Trailing(usize),
    /// The frame id is not one this crate knows in this direction.
    #[error("unknown frame id {0:#06x}")]
    UnknownFrame(u16),
}

fn done(r: &Reader<'_>) -> Result<(), DecodeError> {
    match r.remaining() {
        0 => Ok(()),
        n => Err(DecodeError::Trailing(n)),
    }
}

/// A command from the host.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Command {
    /// Negotiate the protocol version.
    Version {
        desired: u8,
    },
    GetConfigurationValue {
        config_id: u8,
    },
    SetConfigurationValue {
        config_id: u8,
        value: u16,
    },
    AddEndpoint {
        endpoint: u8,
        profile_id: u16,
        device_id: u16,
        device_version: u8,
        input_clusters: Vec<u16>,
        output_clusters: Vec<u16>,
    },
    SetPolicy {
        policy_id: u8,
        decision: u8,
    },
    GetValue {
        value_id: u8,
    },
    SetValue {
        value_id: u8,
        value: Vec<u8>,
    },
    SetManufacturerCode {
        code: u16,
    },
    NetworkInit {
        bitmask: u16,
    },
    NetworkState,
    FormNetwork {
        parameters: NetworkParameters,
    },
    LeaveNetwork {
        options: u8,
    },
    PermitJoining {
        duration: u8,
    },
    GetNetworkParameters,
    GetEui64,
    GetNodeId,
    SetInitialSecurityState {
        state: InitialSecurityState,
    },
    ExportKey {
        context: SecManContext,
    },
    GetNetworkKeyInfo,
    ExportLinkKeyByIndex {
        index: u8,
    },
    ImportLinkKey {
        index: u8,
        ieee: Ieee,
        key: Key,
    },
    EraseKeyTableEntry {
        index: u8,
    },
    ImportTransientKey {
        ieee: Ieee,
        key: Key,
    },
    ClearTransientLinkKeys,
    ClearKeyTable,
    SendUnicast {
        kind: u8,
        destination: NodeId,
        aps: ApsFrame,
        tag: u16,
        payload: Vec<u8>,
    },
    SendBroadcast {
        alias: NodeId,
        destination: NodeId,
        nwk_sequence: u8,
        aps: ApsFrame,
        radius: u8,
        tag: u16,
        payload: Vec<u8>,
    },
    SetConcentrator {
        on: bool,
        kind: u16,
        min_time: u16,
        max_time: u16,
        route_error_threshold: u8,
        delivery_failure_threshold: u8,
        max_hops: u8,
    },
    SetSourceRouteDiscoveryMode {
        mode: u8,
    },
    GetMfgToken {
        token_id: u8,
    },
    SetMfgToken {
        token_id: u8,
        data: Vec<u8>,
    },
    StartScan {
        scan_type: u8,
        channel_mask: u32,
        duration: u8,
    },
    LookupEui64ByNodeId {
        node_id: NodeId,
    },
    SetRadioPower {
        power: i8,
    },
    Echo {
        data: Vec<u8>,
    },
    Nop,
}

impl Command {
    /// The frame id.
    pub fn frame_id(&self) -> u16 {
        match self {
            Command::Version { .. } => id::VERSION,
            Command::GetConfigurationValue { .. } => id::GET_CONFIGURATION_VALUE,
            Command::SetConfigurationValue { .. } => id::SET_CONFIGURATION_VALUE,
            Command::AddEndpoint { .. } => id::ADD_ENDPOINT,
            Command::SetPolicy { .. } => id::SET_POLICY,
            Command::GetValue { .. } => id::GET_VALUE,
            Command::SetValue { .. } => id::SET_VALUE,
            Command::SetManufacturerCode { .. } => id::SET_MANUFACTURER_CODE,
            Command::NetworkInit { .. } => id::NETWORK_INIT,
            Command::NetworkState => id::NETWORK_STATE,
            Command::FormNetwork { .. } => id::FORM_NETWORK,
            Command::LeaveNetwork { .. } => id::LEAVE_NETWORK,
            Command::PermitJoining { .. } => id::PERMIT_JOINING,
            Command::GetNetworkParameters => id::GET_NETWORK_PARAMETERS,
            Command::GetEui64 => id::GET_EUI64,
            Command::GetNodeId => id::GET_NODE_ID,
            Command::SetInitialSecurityState { .. } => id::SET_INITIAL_SECURITY_STATE,
            Command::ExportKey { .. } => id::EXPORT_KEY,
            Command::GetNetworkKeyInfo => id::GET_NETWORK_KEY_INFO,
            Command::ExportLinkKeyByIndex { .. } => id::EXPORT_LINK_KEY_BY_INDEX,
            Command::ImportLinkKey { .. } => id::IMPORT_LINK_KEY,
            Command::EraseKeyTableEntry { .. } => id::ERASE_KEY_TABLE_ENTRY,
            Command::ImportTransientKey { .. } => id::IMPORT_TRANSIENT_KEY,
            Command::ClearTransientLinkKeys => id::CLEAR_TRANSIENT_LINK_KEYS,
            Command::ClearKeyTable => id::CLEAR_KEY_TABLE,
            Command::SendUnicast { .. } => id::SEND_UNICAST,
            Command::SendBroadcast { .. } => id::SEND_BROADCAST,
            Command::SetConcentrator { .. } => id::SET_CONCENTRATOR,
            Command::SetSourceRouteDiscoveryMode { .. } => id::SET_SOURCE_ROUTE_DISCOVERY_MODE,
            Command::GetMfgToken { .. } => id::GET_MFG_TOKEN,
            Command::SetMfgToken { .. } => id::SET_MFG_TOKEN,
            Command::StartScan { .. } => id::START_SCAN,
            Command::LookupEui64ByNodeId { .. } => id::LOOKUP_EUI64_BY_NODE_ID,
            Command::SetRadioPower { .. } => id::SET_RADIO_POWER,
            Command::Echo { .. } => id::ECHO,
            Command::Nop => id::NOP,
        }
    }

    /// Encode the parameters.
    pub fn encode(&self, v: ProtocolVersion) -> Vec<u8> {
        let mut w = Vec::with_capacity(32);
        match self {
            Command::Version { desired } => w.put_u8(*desired),
            Command::GetConfigurationValue { config_id } => w.put_u8(*config_id),
            Command::SetConfigurationValue { config_id, value } => {
                w.put_u8(*config_id);
                w.put_u16(*value);
            }
            Command::AddEndpoint {
                endpoint,
                profile_id,
                device_id,
                device_version,
                input_clusters,
                output_clusters,
            } => {
                w.put_u8(*endpoint);
                w.put_u16(*profile_id);
                w.put_u16(*device_id);
                w.put_u8(*device_version);
                w.put_u8(input_clusters.len() as u8);
                w.put_u8(output_clusters.len() as u8);
                input_clusters.iter().for_each(|c| w.put_u16(*c));
                output_clusters.iter().for_each(|c| w.put_u16(*c));
            }
            Command::SetPolicy {
                policy_id,
                decision,
            } => {
                w.put_u8(*policy_id);
                w.put_u8(*decision);
            }
            Command::GetValue { value_id } => w.put_u8(*value_id),
            Command::SetValue { value_id, value } => {
                w.put_u8(*value_id);
                w.put_u8_prefixed(value);
            }
            Command::SetManufacturerCode { code } => w.put_u16(*code),
            Command::NetworkInit { bitmask } => w.put_u16(*bitmask),
            Command::FormNetwork { parameters } => parameters.write(&mut w),
            Command::LeaveNetwork { options } => {
                if v.sl_status() {
                    w.put_u8(*options);
                }
            }
            Command::PermitJoining { duration } => w.put_u8(*duration),
            Command::SetInitialSecurityState { state } => state.write(&mut w),
            Command::ExportKey { context } => context.write(&mut w),
            Command::ExportLinkKeyByIndex { index } | Command::EraseKeyTableEntry { index } => {
                w.put_u8(*index)
            }
            Command::ImportLinkKey { index, ieee, key } => {
                w.put_u8(*index);
                w.put_ieee(*ieee);
                w.put_key(key);
            }
            Command::ImportTransientKey { ieee, key } => {
                w.put_ieee(*ieee);
                w.put_key(key);
                if !v.sl_status() {
                    w.put_u8(0); // sl_zb_sec_man_flags_t: none
                }
            }
            Command::SendUnicast {
                kind,
                destination,
                aps,
                tag,
                payload,
            } => {
                w.put_u8(*kind);
                w.put_u16(*destination);
                aps.write(&mut w);
                put_tag(&mut w, v, *tag);
                w.put_u8_prefixed(payload);
            }
            Command::SendBroadcast {
                alias,
                destination,
                nwk_sequence,
                aps,
                radius,
                tag,
                payload,
            } => {
                if v.sl_status() {
                    w.put_u16(*alias);
                    w.put_u16(*destination);
                    w.put_u8(*nwk_sequence);
                } else {
                    w.put_u16(*destination);
                }
                aps.write(&mut w);
                w.put_u8(*radius);
                put_tag(&mut w, v, *tag);
                w.put_u8_prefixed(payload);
            }
            Command::SetConcentrator {
                on,
                kind,
                min_time,
                max_time,
                route_error_threshold,
                delivery_failure_threshold,
                max_hops,
            } => {
                w.put_u8(u8::from(*on));
                w.put_u16(*kind);
                w.put_u16(*min_time);
                w.put_u16(*max_time);
                w.put_u8(*route_error_threshold);
                w.put_u8(*delivery_failure_threshold);
                w.put_u8(*max_hops);
            }
            Command::SetSourceRouteDiscoveryMode { mode } => w.put_u8(*mode),
            Command::GetMfgToken { token_id } => w.put_u8(*token_id),
            Command::SetMfgToken { token_id, data } => {
                w.put_u8(*token_id);
                w.put_u8_prefixed(data);
            }
            Command::StartScan {
                scan_type,
                channel_mask,
                duration,
            } => {
                w.put_u8(*scan_type);
                w.put_u32(*channel_mask);
                w.put_u8(*duration);
            }
            Command::LookupEui64ByNodeId { node_id } => w.put_u16(*node_id),
            Command::SetRadioPower { power } => w.put_u8(*power as u8),
            Command::Echo { data } => w.put_u8_prefixed(data),
            Command::NetworkState
            | Command::GetNetworkParameters
            | Command::GetEui64
            | Command::GetNodeId
            | Command::GetNetworkKeyInfo
            | Command::ClearTransientLinkKeys
            | Command::ClearKeyTable
            | Command::Nop => {}
        }
        w
    }

    /// Decode a command (the NCP side).
    pub fn decode(
        v: ProtocolVersion,
        frame_id: u16,
        params: &[u8],
    ) -> Result<Command, DecodeError> {
        let mut r = Reader::new(params);
        let c = match frame_id {
            id::VERSION => Command::Version { desired: r.u8()? },
            id::GET_CONFIGURATION_VALUE => Command::GetConfigurationValue { config_id: r.u8()? },
            id::SET_CONFIGURATION_VALUE => Command::SetConfigurationValue {
                config_id: r.u8()?,
                value: r.u16()?,
            },
            id::ADD_ENDPOINT => {
                let endpoint = r.u8()?;
                let profile_id = r.u16()?;
                let device_id = r.u16()?;
                let device_version = r.u8()?;
                let inputs = usize::from(r.u8()?);
                let outputs = usize::from(r.u8()?);
                Command::AddEndpoint {
                    endpoint,
                    profile_id,
                    device_id,
                    device_version,
                    input_clusters: r.u16_list(inputs)?,
                    output_clusters: r.u16_list(outputs)?,
                }
            }
            id::SET_POLICY => Command::SetPolicy {
                policy_id: r.u8()?,
                decision: r.u8()?,
            },
            id::GET_VALUE => Command::GetValue { value_id: r.u8()? },
            id::SET_VALUE => Command::SetValue {
                value_id: r.u8()?,
                value: r.u8_prefixed()?.to_vec(),
            },
            id::SET_MANUFACTURER_CODE => Command::SetManufacturerCode { code: r.u16()? },
            id::NETWORK_INIT => Command::NetworkInit { bitmask: r.u16()? },
            id::NETWORK_STATE => Command::NetworkState,
            id::FORM_NETWORK => Command::FormNetwork {
                parameters: NetworkParameters::read(&mut r)?,
            },
            id::LEAVE_NETWORK => Command::LeaveNetwork {
                options: if v.sl_status() { r.u8()? } else { 0 },
            },
            id::PERMIT_JOINING => Command::PermitJoining { duration: r.u8()? },
            id::GET_NETWORK_PARAMETERS => Command::GetNetworkParameters,
            id::GET_EUI64 => Command::GetEui64,
            id::GET_NODE_ID => Command::GetNodeId,
            id::SET_INITIAL_SECURITY_STATE => Command::SetInitialSecurityState {
                state: InitialSecurityState::read(&mut r)?,
            },
            id::EXPORT_KEY => Command::ExportKey {
                context: SecManContext::read(&mut r)?,
            },
            id::GET_NETWORK_KEY_INFO => Command::GetNetworkKeyInfo,
            id::EXPORT_LINK_KEY_BY_INDEX => Command::ExportLinkKeyByIndex { index: r.u8()? },
            id::IMPORT_LINK_KEY => Command::ImportLinkKey {
                index: r.u8()?,
                ieee: r.ieee()?,
                key: r.key()?,
            },
            id::ERASE_KEY_TABLE_ENTRY => Command::EraseKeyTableEntry { index: r.u8()? },
            id::IMPORT_TRANSIENT_KEY => {
                let c = Command::ImportTransientKey {
                    ieee: r.ieee()?,
                    key: r.key()?,
                };
                if !v.sl_status() {
                    r.u8()?;
                }
                c
            }
            id::CLEAR_TRANSIENT_LINK_KEYS => Command::ClearTransientLinkKeys,
            id::CLEAR_KEY_TABLE => Command::ClearKeyTable,
            id::SEND_UNICAST => Command::SendUnicast {
                kind: r.u8()?,
                destination: r.u16()?,
                aps: ApsFrame::read(&mut r)?,
                tag: read_tag(&mut r, v)?,
                payload: r.u8_prefixed()?.to_vec(),
            },
            id::SEND_BROADCAST => {
                let (alias, destination, nwk_sequence) = if v.sl_status() {
                    (r.u16()?, r.u16()?, r.u8()?)
                } else {
                    (0, r.u16()?, 0)
                };
                Command::SendBroadcast {
                    alias,
                    destination,
                    nwk_sequence,
                    aps: ApsFrame::read(&mut r)?,
                    radius: r.u8()?,
                    tag: read_tag(&mut r, v)?,
                    payload: r.u8_prefixed()?.to_vec(),
                }
            }
            id::SET_CONCENTRATOR => Command::SetConcentrator {
                on: r.bool()?,
                kind: r.u16()?,
                min_time: r.u16()?,
                max_time: r.u16()?,
                route_error_threshold: r.u8()?,
                delivery_failure_threshold: r.u8()?,
                max_hops: r.u8()?,
            },
            id::SET_SOURCE_ROUTE_DISCOVERY_MODE => {
                Command::SetSourceRouteDiscoveryMode { mode: r.u8()? }
            }
            id::GET_MFG_TOKEN => Command::GetMfgToken { token_id: r.u8()? },
            id::SET_MFG_TOKEN => Command::SetMfgToken {
                token_id: r.u8()?,
                data: r.u8_prefixed()?.to_vec(),
            },
            id::START_SCAN => Command::StartScan {
                scan_type: r.u8()?,
                channel_mask: r.u32()?,
                duration: r.u8()?,
            },
            id::LOOKUP_EUI64_BY_NODE_ID => Command::LookupEui64ByNodeId { node_id: r.u16()? },
            id::SET_RADIO_POWER => Command::SetRadioPower { power: r.i8()? },
            id::ECHO => Command::Echo {
                data: r.u8_prefixed()?.to_vec(),
            },
            id::NOP => Command::Nop,
            other => return Err(DecodeError::UnknownFrame(other)),
        };
        done(&r)?;
        Ok(c)
    }
}

fn put_tag(w: &mut Vec<u8>, v: ProtocolVersion, tag: u16) {
    if v.sl_status() {
        w.put_u16(tag);
    } else {
        w.put_u8(tag as u8);
    }
}

fn read_tag(r: &mut Reader<'_>, v: ProtocolVersion) -> Result<u16, Truncated> {
    if v.sl_status() {
        r.u16()
    } else {
        r.u8().map(u16::from)
    }
}

/// A response from the NCP. Each variant is the response to the
/// [`Command`] of the same frame id.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Response {
    Version {
        protocol_version: u8,
        stack_type: u8,
        stack_version: u16,
    },
    GetConfigurationValue {
        status: Status,
        value: u16,
    },
    /// Every command whose only result is a status.
    Status {
        frame_id: u16,
        status: Status,
    },
    GetValue {
        status: Status,
        value: Vec<u8>,
    },
    NetworkState {
        state: u8,
    },
    GetNetworkParameters {
        status: Status,
        node_type: u8,
        parameters: NetworkParameters,
    },
    GetEui64 {
        ieee: Ieee,
    },
    GetNodeId {
        node_id: NodeId,
    },
    ExportKey {
        status: Status,
        key: Key,
        context: SecManContext,
    },
    GetNetworkKeyInfo {
        status: Status,
        info: NetworkKeyInfo,
    },
    ExportLinkKeyByIndex {
        status: Status,
        context: SecManContext,
        key: Key,
        metadata: ApsKeyMetadata,
    },
    /// `sendUnicast` and `sendBroadcast`.
    Sent {
        frame_id: u16,
        status: Status,
        aps_sequence: u8,
    },
    SetSourceRouteDiscoveryMode {
        remaining_ms: u32,
    },
    GetMfgToken {
        data: Vec<u8>,
    },
    LookupEui64ByNodeId {
        status: Status,
        ieee: Ieee,
    },
    Echo {
        data: Vec<u8>,
    },
    /// Commands with no return values.
    Empty {
        frame_id: u16,
    },
    /// The NCP did not understand the command (`invalidCommand`).
    InvalidCommand {
        reason: u8,
    },
}

/// How each command's response is laid out.
enum Shape {
    /// Status in the version's width (`EmberStatus` before 14).
    Status,
    /// `EzspStatus`: one byte before 14, `sl_status_t` after.
    EzspStatus,
    /// `sl_status_t` in every version.
    SlStatus,
    Empty,
    Special,
}

fn shape(frame_id: u16) -> Option<Shape> {
    Some(match frame_id {
        id::SET_CONFIGURATION_VALUE | id::ADD_ENDPOINT | id::SET_POLICY | id::SET_VALUE => {
            Shape::EzspStatus
        }
        id::NETWORK_INIT
        | id::FORM_NETWORK
        | id::LEAVE_NETWORK
        | id::PERMIT_JOINING
        | id::SET_INITIAL_SECURITY_STATE
        | id::ERASE_KEY_TABLE_ENTRY
        | id::CLEAR_KEY_TABLE
        | id::SET_CONCENTRATOR
        | id::SET_MFG_TOKEN
        | id::SET_RADIO_POWER => Shape::Status,
        id::IMPORT_LINK_KEY | id::IMPORT_TRANSIENT_KEY | id::START_SCAN => Shape::SlStatus,
        id::SET_MANUFACTURER_CODE | id::CLEAR_TRANSIENT_LINK_KEYS | id::NOP => Shape::Empty,
        id::VERSION
        | id::GET_CONFIGURATION_VALUE
        | id::GET_VALUE
        | id::NETWORK_STATE
        | id::GET_NETWORK_PARAMETERS
        | id::GET_EUI64
        | id::GET_NODE_ID
        | id::EXPORT_KEY
        | id::GET_NETWORK_KEY_INFO
        | id::EXPORT_LINK_KEY_BY_INDEX
        | id::SEND_UNICAST
        | id::SEND_BROADCAST
        | id::SET_SOURCE_ROUTE_DISCOVERY_MODE
        | id::GET_MFG_TOKEN
        | id::LOOKUP_EUI64_BY_NODE_ID
        | id::ECHO
        | id::INVALID_COMMAND => Shape::Special,
        _ => return None,
    })
}

fn read_ezsp_status(r: &mut Reader<'_>, v: ProtocolVersion) -> Result<Status, Truncated> {
    Status::read(r, v)
}

impl Response {
    /// The frame id this response answers.
    pub fn frame_id(&self) -> u16 {
        match self {
            Response::Version { .. } => id::VERSION,
            Response::GetConfigurationValue { .. } => id::GET_CONFIGURATION_VALUE,
            Response::Status { frame_id, .. }
            | Response::Sent { frame_id, .. }
            | Response::Empty { frame_id } => *frame_id,
            Response::GetValue { .. } => id::GET_VALUE,
            Response::NetworkState { .. } => id::NETWORK_STATE,
            Response::GetNetworkParameters { .. } => id::GET_NETWORK_PARAMETERS,
            Response::GetEui64 { .. } => id::GET_EUI64,
            Response::GetNodeId { .. } => id::GET_NODE_ID,
            Response::ExportKey { .. } => id::EXPORT_KEY,
            Response::GetNetworkKeyInfo { .. } => id::GET_NETWORK_KEY_INFO,
            Response::ExportLinkKeyByIndex { .. } => id::EXPORT_LINK_KEY_BY_INDEX,
            Response::SetSourceRouteDiscoveryMode { .. } => id::SET_SOURCE_ROUTE_DISCOVERY_MODE,
            Response::GetMfgToken { .. } => id::GET_MFG_TOKEN,
            Response::LookupEui64ByNodeId { .. } => id::LOOKUP_EUI64_BY_NODE_ID,
            Response::Echo { .. } => id::ECHO,
            Response::InvalidCommand { .. } => id::INVALID_COMMAND,
        }
    }

    /// The status, for responses that carry one.
    pub fn status(&self) -> Option<Status> {
        match self {
            Response::GetConfigurationValue { status, .. }
            | Response::Status { status, .. }
            | Response::GetValue { status, .. }
            | Response::GetNetworkParameters { status, .. }
            | Response::ExportKey { status, .. }
            | Response::GetNetworkKeyInfo { status, .. }
            | Response::ExportLinkKeyByIndex { status, .. }
            | Response::Sent { status, .. }
            | Response::LookupEui64ByNodeId { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Decode a response (the host side).
    pub fn decode(
        v: ProtocolVersion,
        frame_id: u16,
        params: &[u8],
    ) -> Result<Response, DecodeError> {
        let mut r = Reader::new(params);
        let shape = shape(frame_id).ok_or(DecodeError::UnknownFrame(frame_id))?;
        let response = match shape {
            Shape::Status => Response::Status {
                frame_id,
                status: Status::read(&mut r, v)?,
            },
            Shape::EzspStatus => Response::Status {
                frame_id,
                status: read_ezsp_status(&mut r, v)?,
            },
            Shape::SlStatus => Response::Status {
                frame_id,
                status: Status::read_sl(&mut r)?,
            },
            Shape::Empty => Response::Empty { frame_id },
            Shape::Special => match frame_id {
                id::VERSION => Response::Version {
                    protocol_version: r.u8()?,
                    stack_type: r.u8()?,
                    stack_version: r.u16()?,
                },
                id::GET_CONFIGURATION_VALUE => Response::GetConfigurationValue {
                    status: read_ezsp_status(&mut r, v)?,
                    value: r.u16()?,
                },
                id::GET_VALUE => Response::GetValue {
                    status: read_ezsp_status(&mut r, v)?,
                    value: r.u8_prefixed()?.to_vec(),
                },
                id::NETWORK_STATE => Response::NetworkState { state: r.u8()? },
                id::GET_NETWORK_PARAMETERS => Response::GetNetworkParameters {
                    status: Status::read(&mut r, v)?,
                    node_type: r.u8()?,
                    parameters: NetworkParameters::read(&mut r)?,
                },
                id::GET_EUI64 => Response::GetEui64 { ieee: r.ieee()? },
                id::GET_NODE_ID => Response::GetNodeId { node_id: r.u16()? },
                id::EXPORT_KEY => {
                    if v.sl_status() {
                        let status = Status::read_sl(&mut r)?;
                        let key = r.key()?;
                        Response::ExportKey {
                            status,
                            key,
                            context: SecManContext::read(&mut r)?,
                        }
                    } else {
                        let key = r.key()?;
                        Response::ExportKey {
                            status: Status::read_sl(&mut r)?,
                            key,
                            context: SecManContext::default(),
                        }
                    }
                }
                id::GET_NETWORK_KEY_INFO => Response::GetNetworkKeyInfo {
                    status: Status::read_sl(&mut r)?,
                    info: NetworkKeyInfo::read(&mut r)?,
                },
                id::EXPORT_LINK_KEY_BY_INDEX => {
                    if v.sl_status() {
                        let status = Status::read_sl(&mut r)?;
                        let context = SecManContext::read(&mut r)?;
                        let key = r.key()?;
                        Response::ExportLinkKeyByIndex {
                            status,
                            context,
                            key,
                            metadata: ApsKeyMetadata::read(&mut r)?,
                        }
                    } else {
                        let ieee = r.ieee()?;
                        let key = r.key()?;
                        let metadata = ApsKeyMetadata::read(&mut r)?;
                        let status = Status::read_sl(&mut r)?;
                        let context = SecManContext {
                            core_key_type: crate::ids::key_type::APP_LINK,
                            ieee,
                            ..Default::default()
                        };
                        Response::ExportLinkKeyByIndex {
                            status,
                            context,
                            key,
                            metadata,
                        }
                    }
                }
                id::SEND_UNICAST | id::SEND_BROADCAST => Response::Sent {
                    frame_id,
                    status: Status::read(&mut r, v)?,
                    aps_sequence: r.u8()?,
                },
                id::SET_SOURCE_ROUTE_DISCOVERY_MODE => Response::SetSourceRouteDiscoveryMode {
                    remaining_ms: r.u32()?,
                },
                id::GET_MFG_TOKEN => Response::GetMfgToken {
                    data: r.u8_prefixed()?.to_vec(),
                },
                id::LOOKUP_EUI64_BY_NODE_ID => Response::LookupEui64ByNodeId {
                    status: Status::read(&mut r, v)?,
                    ieee: r.ieee()?,
                },
                id::ECHO => Response::Echo {
                    data: r.u8_prefixed()?.to_vec(),
                },
                id::INVALID_COMMAND => Response::InvalidCommand { reason: r.u8()? },
                other => return Err(DecodeError::UnknownFrame(other)),
            },
        };
        // The NCP may append fields this version of the crate does not
        // know; trailing bytes are tolerated in responses.
        Ok(response)
    }

    /// Encode the parameters (the NCP side). Statuses are written in their
    /// own width, so build them for the negotiated version.
    pub fn encode(&self, v: ProtocolVersion) -> Vec<u8> {
        let mut w = Vec::with_capacity(32);
        match self {
            Response::Version {
                protocol_version,
                stack_type,
                stack_version,
            } => {
                w.put_u8(*protocol_version);
                w.put_u8(*stack_type);
                w.put_u16(*stack_version);
            }
            Response::GetConfigurationValue { status, value } => {
                status.write(&mut w);
                w.put_u16(*value);
            }
            Response::Status { status, .. } => status.write(&mut w),
            Response::GetValue { status, value } => {
                status.write(&mut w);
                w.put_u8_prefixed(value);
            }
            Response::NetworkState { state } => w.put_u8(*state),
            Response::GetNetworkParameters {
                status,
                node_type,
                parameters,
            } => {
                status.write(&mut w);
                w.put_u8(*node_type);
                parameters.write(&mut w);
            }
            Response::GetEui64 { ieee } => w.put_ieee(*ieee),
            Response::GetNodeId { node_id } => w.put_u16(*node_id),
            Response::ExportKey {
                status,
                key,
                context,
            } => {
                if v.sl_status() {
                    status.write(&mut w);
                    w.put_key(key);
                    context.write(&mut w);
                } else {
                    w.put_key(key);
                    status.write(&mut w);
                }
            }
            Response::GetNetworkKeyInfo { status, info } => {
                status.write(&mut w);
                info.write(&mut w);
            }
            Response::ExportLinkKeyByIndex {
                status,
                context,
                key,
                metadata,
            } => {
                if v.sl_status() {
                    status.write(&mut w);
                    context.write(&mut w);
                    w.put_key(key);
                    metadata.write(&mut w);
                } else {
                    w.put_ieee(context.ieee);
                    w.put_key(key);
                    metadata.write(&mut w);
                    status.write(&mut w);
                }
            }
            Response::Sent {
                status,
                aps_sequence,
                ..
            } => {
                status.write(&mut w);
                w.put_u8(*aps_sequence);
            }
            Response::SetSourceRouteDiscoveryMode { remaining_ms } => w.put_u32(*remaining_ms),
            Response::GetMfgToken { data } | Response::Echo { data } => w.put_u8_prefixed(data),
            Response::LookupEui64ByNodeId { status, ieee } => {
                status.write(&mut w);
                w.put_ieee(*ieee);
            }
            Response::Empty { .. } => {}
            Response::InvalidCommand { reason } => w.put_u8(*reason),
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_command() -> Vec<Command> {
        let aps = ApsFrame {
            profile_id: 0x0104,
            cluster_id: 6,
            source_endpoint: 1,
            destination_endpoint: 1,
            options: 0x140,
            group_id: 0,
            sequence: 9,
        };
        vec![
            Command::Version { desired: 13 },
            Command::GetConfigurationValue { config_id: 0x1E },
            Command::SetConfigurationValue {
                config_id: 0x11,
                value: 32,
            },
            Command::AddEndpoint {
                endpoint: 1,
                profile_id: 0x0104,
                device_id: 5,
                device_version: 0,
                input_clusters: vec![0, 3],
                output_clusters: vec![6],
            },
            Command::SetPolicy {
                policy_id: 5,
                decision: 0x51,
            },
            Command::GetValue { value_id: 0x11 },
            Command::SetValue {
                value_id: 0x23,
                value: vec![1, 2, 3, 4],
            },
            Command::SetManufacturerCode { code: 0x1049 },
            Command::NetworkInit { bitmask: 3 },
            Command::NetworkState,
            Command::FormNetwork {
                parameters: NetworkParameters {
                    radio_channel: 15,
                    ..Default::default()
                },
            },
            Command::PermitJoining { duration: 254 },
            Command::GetNetworkParameters,
            Command::GetEui64,
            Command::GetNodeId,
            Command::SetInitialSecurityState {
                state: InitialSecurityState {
                    bitmask: 0x0B84,
                    ..Default::default()
                },
            },
            Command::ExportKey {
                context: SecManContext::for_key(1),
            },
            Command::GetNetworkKeyInfo,
            Command::ExportLinkKeyByIndex { index: 3 },
            Command::ImportLinkKey {
                index: 1,
                ieee: Ieee(42),
                key: Key([7; 16]),
            },
            Command::EraseKeyTableEntry { index: 2 },
            Command::ImportTransientKey {
                ieee: Ieee(9),
                key: Key::ZIGBEE_ALLIANCE_09,
            },
            Command::ClearTransientLinkKeys,
            Command::ClearKeyTable,
            Command::SendUnicast {
                kind: 0,
                destination: 0x1234,
                aps,
                tag: 7,
                payload: vec![0x10, 1, 0, 0, 0],
            },
            Command::SetConcentrator {
                on: true,
                kind: 0xFFF9,
                min_time: 5,
                max_time: 60,
                route_error_threshold: 3,
                delivery_failure_threshold: 1,
                max_hops: 0,
            },
            Command::SetSourceRouteDiscoveryMode { mode: 2 },
            Command::GetMfgToken { token_id: 0x0C },
            Command::SetMfgToken {
                token_id: 0x0C,
                data: vec![1; 8],
            },
            Command::StartScan {
                scan_type: 0,
                channel_mask: 0x07FF_F800,
                duration: 3,
            },
            Command::LookupEui64ByNodeId { node_id: 0x1234 },
            Command::SetRadioPower { power: -4 },
            Command::Echo {
                data: vec![1, 2, 3],
            },
            Command::Nop,
        ]
    }

    #[test]
    fn commands_round_trip_in_both_layouts() {
        for v in [ProtocolVersion(13), ProtocolVersion(16)] {
            for c in every_command() {
                let bytes = c.encode(v);
                assert_eq!(
                    Command::decode(v, c.frame_id(), &bytes),
                    Ok(c.clone()),
                    "{c:?} v{}",
                    v.0
                );
            }
            let b = Command::SendBroadcast {
                alias: if v.sl_status() { 1 } else { 0 },
                destination: 0xFFFC,
                nwk_sequence: 0,
                aps: ApsFrame::default(),
                radius: 30,
                tag: 1,
                payload: vec![0, 1, 2],
            };
            assert_eq!(Command::decode(v, b.frame_id(), &b.encode(v)), Ok(b));
        }
    }

    #[test]
    fn unicast_layout_differs_by_version() {
        let c = Command::SendUnicast {
            kind: 0,
            destination: 1,
            aps: ApsFrame::default(),
            tag: 0x0102,
            payload: vec![],
        };
        assert_eq!(
            c.encode(ProtocolVersion(13)).len() + 1,
            c.encode(ProtocolVersion(14)).len()
        );
    }

    #[test]
    fn responses_round_trip() {
        for v in [ProtocolVersion(13), ProtocolVersion(18)] {
            let responses = vec![
                Response::Version {
                    protocol_version: 13,
                    stack_type: 2,
                    stack_version: 0x7440,
                },
                Response::GetConfigurationValue {
                    status: Status::ok(v),
                    value: 12,
                },
                Response::Status {
                    frame_id: id::FORM_NETWORK,
                    status: Status::ok(v),
                },
                Response::Status {
                    frame_id: id::IMPORT_TRANSIENT_KEY,
                    status: Status::SL_OK,
                },
                Response::GetValue {
                    status: Status::ok(v),
                    value: vec![1, 2, 3, 4, 5, 6, 7],
                },
                Response::NetworkState { state: 2 },
                Response::GetEui64 { ieee: Ieee(0x1122) },
                Response::GetNodeId { node_id: 0 },
                Response::GetNetworkKeyInfo {
                    status: Status::SL_OK,
                    info: NetworkKeyInfo {
                        network_key_set: true,
                        network_key_frame_counter: 99,
                        ..Default::default()
                    },
                },
                Response::Sent {
                    frame_id: id::SEND_UNICAST,
                    status: Status::ok(v),
                    aps_sequence: 3,
                },
                Response::Empty { frame_id: id::NOP },
                Response::InvalidCommand { reason: 0x58 },
            ];
            for resp in responses {
                assert_eq!(
                    Response::decode(v, resp.frame_id(), &resp.encode(v)),
                    Ok(resp.clone()),
                    "{resp:?}"
                );
            }
            let export = Response::ExportLinkKeyByIndex {
                status: Status::SL_OK,
                context: SecManContext {
                    core_key_type: crate::ids::key_type::APP_LINK,
                    ieee: Ieee(77),
                    ..Default::default()
                },
                key: Key([3; 16]),
                metadata: ApsKeyMetadata {
                    outgoing_frame_counter: 5,
                    ..Default::default()
                },
            };
            assert_eq!(
                Response::decode(v, export.frame_id(), &export.encode(v)),
                Ok(export)
            );
        }
    }
}
