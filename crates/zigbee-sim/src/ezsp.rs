//! The EZSP front-end: an EmberZNet NCP over ASH.

use std::collections::BTreeMap;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use zigbee_ash::{Config as AshConfig, Event as AshEvent, Link, LinkEvent, Role};
use zigbee_ezsp::frame::{self, Format, CONTROL_ASYNC_CALLBACK, CONTROL_RESPONSE};
use zigbee_ezsp::ids::{self, frame as fid, key_type, mfg_token, tc_decision, value};
use zigbee_ezsp::types::{
    ApsKeyMetadata, InitialSecurityState, NetworkKeyInfo, NetworkParameters, SecManContext,
    StackVersion,
};
use zigbee_ezsp::{ApsFrame, Callback, Command, ProtocolVersion, Response, RxPacketInfo, Status};
use zigbee_primitives::{Ieee, Key};

use crate::device::Outgoing;
use crate::mesh::{Mesh, MeshEvent, Scheduled, SimNetwork};
use crate::ncp::{lock, SimError, Simulator};

/// The EmberZNet version an EZSP protocol version implies.
pub(crate) fn stack_version(protocol: u8) -> StackVersion {
    if protocol >= 14 {
        StackVersion {
            build: 75,
            major: 8,
            minor: 0,
            patch: 2,
            special: 0,
            kind: 0xAA,
        }
    } else {
        StackVersion {
            build: 380,
            major: 7,
            minor: 4,
            patch: 4,
            special: 0,
            kind: 0xAA,
        }
    }
}

pub(crate) struct EzspFront {
    pub protocol: u8,
    pub stack_version: StackVersion,
    pub key_table_size: u16,
    pub security: Option<InitialSecurityState>,
    pub key_table: BTreeMap<u16, (Ieee, Key)>,
    pub custom_eui: Option<Ieee>,
    pub custom_eui_pending: Option<Ieee>,
    pub pending_frame_counter: Option<u32>,
    pub endpoints: Vec<u8>,
}

impl EzspFront {
    pub fn new(protocol: u8, key_table_size: u16) -> EzspFront {
        EzspFront {
            protocol,
            stack_version: stack_version(protocol),
            key_table_size,
            security: None,
            key_table: BTreeMap::new(),
            custom_eui: None,
            custom_eui_pending: None,
            pending_frame_counter: None,
            endpoints: Vec::new(),
        }
    }

    /// The NCP reset (RST): the stack restarts, the stored network stays.
    fn reset(&mut self, mesh: &mut Mesh) {
        mesh.reset();
        self.endpoints.clear();
        self.security = None;
        if let Some(eui) = self.custom_eui_pending.take() {
            self.custom_eui = Some(eui);
        }
        mesh.ieee_override = self.custom_eui;
    }

    fn set_tc_policy(mesh: &mut Mesh, decision: u8) {
        mesh.allow_joins = decision & tc_decision::ALLOW_JOINS != 0;
        mesh.install_code_only = decision & tc_decision::JOINS_USE_INSTALL_CODE_KEY != 0;
    }

    fn translate(&self, event: MeshEvent) -> Vec<Callback> {
        match event {
            MeshEvent::Joined {
                nwk,
                ieee,
                end_device,
                key_exchange,
            } => {
                let mut out = vec![Callback::TrustCenterJoin {
                    node_id: nwk,
                    ieee,
                    status: ids::device_update::UNSECURED_JOIN,
                    decision: ids::join_decision::USE_PRECONFIGURED_KEY,
                    parent: 0,
                }];
                if let Some(sleepy) = end_device {
                    out.push(Callback::ChildJoin {
                        index: 0,
                        joining: true,
                        node_id: nwk,
                        ieee,
                        node_type: if sleepy {
                            ids::node_type::SLEEPY_END_DEVICE
                        } else {
                            ids::node_type::END_DEVICE
                        },
                    });
                }
                if key_exchange {
                    out.push(Callback::ZigbeeKeyEstablishment {
                        partner: ieee,
                        status: ids::key_status::TC_RESPONDED_TO_KEY_REQUEST,
                    });
                }
                out
            }
            MeshEvent::Left { nwk, ieee } => vec![Callback::TrustCenterJoin {
                node_id: nwk,
                ieee,
                status: ids::device_update::DEVICE_LEFT,
                decision: ids::join_decision::NO_ACTION,
                parent: 0,
            }],
            MeshEvent::Incoming {
                nwk,
                ieee,
                lqi,
                rssi,
                aps_seq,
                out,
            } => {
                let v = ProtocolVersion(self.protocol);
                vec![Callback::IncomingMessage {
                    kind: ids::incoming::UNICAST,
                    aps: ApsFrame {
                        profile_id: out.profile,
                        cluster_id: out.cluster,
                        source_endpoint: out.source_endpoint,
                        destination_endpoint: out.destination_endpoint,
                        options: 0,
                        group_id: 0,
                        sequence: aps_seq,
                    },
                    info: RxPacketInfo {
                        sender: nwk,
                        sender_ieee: v.sl_status().then_some(ieee),
                        binding_index: 0xFF,
                        address_index: 0xFF,
                        last_hop_lqi: lqi,
                        last_hop_rssi: rssi,
                        last_hop_timestamp: 0,
                    },
                    payload: out.payload,
                }]
            }
        }
    }

    fn stack_status(&self, mesh: &Mesh, status: Status) {
        mesh.schedule(
            Duration::from_millis(5),
            Scheduled::Ezsp(Callback::StackStatus { status }),
        );
    }

    fn command(&mut self, mesh: &mut Mesh, v: ProtocolVersion, command: Command) -> Response {
        let ok = Status::ok(v);
        let status = |frame_id: u16, status: Status| Response::Status { frame_id, status };
        match command {
            Command::Version { desired } => Response::Version {
                protocol_version: if desired == self.protocol {
                    desired
                } else {
                    self.protocol
                },
                stack_type: ids::STACK_TYPE_MESH,
                stack_version: (u16::from(self.stack_version.major) << 12)
                    | (u16::from(self.stack_version.minor & 0xF) << 8)
                    | (u16::from(self.stack_version.patch & 0xF) << 4)
                    | u16::from(self.stack_version.special & 0xF),
            },
            Command::GetConfigurationValue { config_id } => Response::GetConfigurationValue {
                status: ok,
                value: if config_id == ids::config::KEY_TABLE_SIZE {
                    self.key_table_size
                } else {
                    0
                },
            },
            Command::SetConfigurationValue { .. } => status(fid::SET_CONFIGURATION_VALUE, ok),
            Command::AddEndpoint { endpoint, .. } => {
                if self.endpoints.contains(&endpoint) {
                    status(fid::ADD_ENDPOINT, Status::invalid_call(v))
                } else {
                    self.endpoints.push(endpoint);
                    status(fid::ADD_ENDPOINT, ok)
                }
            }
            Command::SetPolicy {
                policy_id,
                decision,
            } => {
                if policy_id == ids::policy::TRUST_CENTER {
                    Self::set_tc_policy(mesh, decision);
                }
                status(fid::SET_POLICY, ok)
            }
            Command::GetValue { value_id } => match value_id {
                value::VERSION_INFO => Response::GetValue {
                    status: ok,
                    value: self.stack_version.to_bytes(),
                },
                _ => Response::GetValue {
                    status: Status::invalid_parameter(v),
                    value: vec![],
                },
            },
            Command::SetValue { value_id, value } => {
                if value_id == value::NWK_FRAME_COUNTER {
                    if let [a, b, c, d] = value.as_slice() {
                        self.pending_frame_counter = Some(u32::from_le_bytes([*a, *b, *c, *d]));
                    }
                }
                status(fid::SET_VALUE, ok)
            }
            Command::SetManufacturerCode { .. } => Response::Empty {
                frame_id: fid::SET_MANUFACTURER_CODE,
            },
            Command::NetworkInit { .. } => {
                if mesh.up {
                    status(fid::NETWORK_INIT, Status::invalid_call(v))
                } else if mesh.network.is_some() {
                    mesh.up = true;
                    self.stack_status(mesh, Status::network_up(v));
                    status(fid::NETWORK_INIT, ok)
                } else {
                    status(fid::NETWORK_INIT, Status::not_joined(v))
                }
            }
            Command::NetworkState => Response::NetworkState {
                state: if mesh.up {
                    ids::network_status::JOINED_NETWORK
                } else {
                    ids::network_status::NO_NETWORK
                },
            },
            Command::FormNetwork { parameters } => {
                let Some(security) = self.security else {
                    return status(fid::FORM_NETWORK, Status::invalid_call(v));
                };
                if mesh.network.is_some() {
                    return status(fid::FORM_NETWORK, Status::invalid_call(v));
                }
                mesh.form(SimNetwork {
                    channel: parameters.radio_channel,
                    pan_id: parameters.pan_id,
                    extended_pan_id: parameters.extended_pan_id,
                    nwk_update_id: parameters.nwk_update_id,
                    channel_mask: parameters.channels,
                    network_key: security.network_key,
                    key_sequence: security.network_key_sequence,
                    tc_root: security.preconfigured_key,
                    frame_counter: self.pending_frame_counter.take().unwrap_or(0),
                });
                self.stack_status(mesh, Status::network_up(v));
                status(fid::FORM_NETWORK, ok)
            }
            Command::LeaveNetwork { .. } => {
                if !mesh.leave() {
                    return status(fid::LEAVE_NETWORK, Status::invalid_call(v));
                }
                self.stack_status(mesh, Status::network_down(v));
                status(fid::LEAVE_NETWORK, ok)
            }
            Command::PermitJoining { duration } => {
                if !mesh.up {
                    return status(fid::PERMIT_JOINING, Status::network_down(v));
                }
                mesh.permit(u16::from(duration));
                let s = if duration == 0 {
                    Status::network_closed(v)
                } else {
                    Status::network_opened(v)
                };
                mesh.schedule(
                    Duration::ZERO,
                    Scheduled::Ezsp(Callback::StackStatus { status: s }),
                );
                status(fid::PERMIT_JOINING, ok)
            }
            Command::GetNetworkParameters => match &mesh.network {
                Some(n) if mesh.up => Response::GetNetworkParameters {
                    status: ok,
                    node_type: ids::node_type::COORDINATOR,
                    parameters: NetworkParameters {
                        extended_pan_id: n.extended_pan_id,
                        pan_id: n.pan_id,
                        radio_tx_power: 5,
                        radio_channel: n.channel,
                        join_method: 0,
                        nwk_manager_id: 0,
                        nwk_update_id: n.nwk_update_id,
                        channels: n.channel_mask,
                    },
                },
                _ => Response::GetNetworkParameters {
                    status: Status::not_joined(v),
                    node_type: 0,
                    parameters: NetworkParameters::default(),
                },
            },
            Command::GetEui64 => Response::GetEui64 { ieee: mesh.ieee() },
            Command::GetNodeId => Response::GetNodeId {
                node_id: if mesh.up { 0x0000 } else { 0xFFFE },
            },
            Command::SetInitialSecurityState { state } => {
                self.security = Some(state);
                status(fid::SET_INITIAL_SECURITY_STATE, ok)
            }
            Command::ExportKey { context } => {
                let key = match (context.core_key_type, &mesh.network) {
                    (key_type::NETWORK, Some(n)) => Some(n.network_key),
                    (key_type::TC_LINK, Some(n)) => Some(n.tc_root),
                    _ => None,
                };
                match key {
                    Some(key) => Response::ExportKey {
                        status: Status::SL_OK,
                        key,
                        context,
                    },
                    None => Response::ExportKey {
                        status: Status::SL_NOT_FOUND,
                        key: Key::default(),
                        context,
                    },
                }
            }
            Command::GetNetworkKeyInfo => match &mesh.network {
                Some(n) => Response::GetNetworkKeyInfo {
                    status: Status::SL_OK,
                    info: NetworkKeyInfo {
                        network_key_set: true,
                        alternate_network_key_set: false,
                        network_key_sequence: n.key_sequence,
                        alternate_network_key_sequence: 0,
                        network_key_frame_counter: n.frame_counter,
                    },
                },
                None => Response::GetNetworkKeyInfo {
                    status: Status::SL_NOT_FOUND,
                    info: NetworkKeyInfo::default(),
                },
            },
            Command::ExportLinkKeyByIndex { index } => {
                match self.key_table.get(&u16::from(index)) {
                    Some((ieee, key)) => Response::ExportLinkKeyByIndex {
                        status: Status::SL_OK,
                        context: SecManContext {
                            core_key_type: key_type::APP_LINK,
                            key_index: index,
                            ieee: *ieee,
                            ..Default::default()
                        },
                        key: *key,
                        metadata: ApsKeyMetadata::default(),
                    },
                    None => Response::ExportLinkKeyByIndex {
                        status: Status::SL_NOT_FOUND,
                        context: SecManContext::default(),
                        key: Key::default(),
                        metadata: ApsKeyMetadata::default(),
                    },
                }
            }
            Command::ImportLinkKey { index, ieee, key } => {
                if u16::from(index) >= self.key_table_size {
                    return status(
                        fid::IMPORT_LINK_KEY,
                        Status {
                            raw: 0x0027,
                            legacy: false,
                        },
                    );
                }
                self.key_table.insert(u16::from(index), (ieee, key));
                status(fid::IMPORT_LINK_KEY, Status::SL_OK)
            }
            Command::EraseKeyTableEntry { index } => {
                self.key_table.remove(&u16::from(index));
                status(fid::ERASE_KEY_TABLE_ENTRY, ok)
            }
            Command::ClearKeyTable => {
                self.key_table.clear();
                status(fid::CLEAR_KEY_TABLE, ok)
            }
            Command::ImportTransientKey { ieee, key } => {
                if ieee == Ieee::ZERO {
                    mesh.well_known_ok = key == Key::ZIGBEE_ALLIANCE_09;
                } else {
                    mesh.join_keys.insert(ieee, key);
                }
                if mesh.permit_open() {
                    mesh.schedule_joins();
                }
                status(fid::IMPORT_TRANSIENT_KEY, Status::SL_OK)
            }
            Command::ClearTransientLinkKeys => {
                mesh.well_known_ok = false;
                mesh.join_keys.clear();
                Response::Empty {
                    frame_id: fid::CLEAR_TRANSIENT_LINK_KEYS,
                }
            }
            Command::SendUnicast {
                destination,
                aps,
                tag,
                payload,
                ..
            } => {
                if !mesh.up {
                    return Response::Sent {
                        frame_id: fid::SEND_UNICAST,
                        status: Status::network_down(v),
                        aps_sequence: 0,
                    };
                }
                let out = Outgoing {
                    profile: aps.profile_id,
                    cluster: aps.cluster_id,
                    source_endpoint: aps.source_endpoint,
                    destination_endpoint: aps.destination_endpoint,
                    payload,
                };
                let delivered = mesh.unicast(destination, &out);
                let aps_sequence = mesh.next_aps_seq();
                let sent = Callback::MessageSent {
                    status: if delivered {
                        Status::ok(v)
                    } else {
                        Status::delivery_failed(v)
                    },
                    kind: ids::outgoing::DIRECT,
                    destination,
                    aps,
                    tag,
                };
                mesh.schedule(Duration::from_millis(2), Scheduled::Ezsp(sent));
                Response::Sent {
                    frame_id: fid::SEND_UNICAST,
                    status: ok,
                    aps_sequence,
                }
            }
            Command::SendBroadcast { aps, payload, .. } => {
                if mesh.up && aps.profile_id == 0 && aps.cluster_id == 0x0038 {
                    // A network update reaches the coordinator too.
                    let _ = mesh.network_update(payload.get(1..).unwrap_or(&[]));
                }
                Response::Sent {
                    frame_id: fid::SEND_BROADCAST,
                    status: if mesh.up { ok } else { Status::network_down(v) },
                    aps_sequence: mesh.next_aps_seq(),
                }
            }
            Command::SetConcentrator { .. } => status(fid::SET_CONCENTRATOR, ok),
            Command::SetSourceRouteDiscoveryMode { .. } => Response::SetSourceRouteDiscoveryMode {
                remaining_ms: 10_000,
            },
            Command::GetMfgToken { token_id } => Response::GetMfgToken {
                data: match token_id {
                    mfg_token::CUSTOM_EUI_64 => self
                        .custom_eui
                        .or(self.custom_eui_pending)
                        .map(|e| e.to_le_bytes().to_vec())
                        .unwrap_or_else(|| vec![0xFF; 8]),
                    mfg_token::STRING => {
                        b"zigbee-bridge sim\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0".to_vec()
                    }
                    mfg_token::BOARD_NAME => b"ezsp-ncp-sim\0\0\0\0".to_vec(),
                    _ => vec![0xFF; 2],
                },
            },
            Command::SetMfgToken { token_id, data } => {
                if token_id == mfg_token::CUSTOM_EUI_64 {
                    if self.custom_eui.is_some() || self.custom_eui_pending.is_some() {
                        return status(fid::SET_MFG_TOKEN, Status::invalid_call(v));
                    }
                    if let Ok(bytes) = <[u8; 8]>::try_from(data.as_slice()) {
                        self.custom_eui_pending = Some(Ieee::from_le_bytes(bytes));
                    }
                }
                status(fid::SET_MFG_TOKEN, ok)
            }
            Command::StartScan {
                scan_type,
                channel_mask,
                ..
            } => {
                if scan_type == ids::scan_type::ACTIVE {
                    for n in mesh
                        .neighbors
                        .iter()
                        .filter(|n| channel_mask & (1 << n.channel) != 0)
                    {
                        mesh.schedule(
                            Duration::from_millis(u64::from(n.channel)),
                            Scheduled::Ezsp(Callback::NetworkFound {
                                channel: n.channel,
                                pan_id: n.pan_id,
                                extended_pan_id: n.extended_pan_id,
                                allowing_join: n.permit_joining,
                                stack_profile: 2,
                                nwk_update_id: 0,
                                lqi: 150,
                                rssi: -70,
                            }),
                        );
                    }
                    mesh.schedule(
                        Duration::from_millis(40),
                        Scheduled::Ezsp(Callback::ScanComplete {
                            channel: 26,
                            status: Status::ok(v),
                        }),
                    );
                }
                if scan_type == ids::scan_type::ENERGY {
                    let mut last = 11;
                    for (channel, max_rssi) in Mesh::energy(channel_mask) {
                        mesh.schedule(
                            Duration::from_millis(u64::from(channel)),
                            Scheduled::Ezsp(Callback::EnergyScanResult { channel, max_rssi }),
                        );
                        last = channel;
                    }
                    mesh.schedule(
                        Duration::from_millis(40),
                        Scheduled::Ezsp(Callback::ScanComplete {
                            channel: last,
                            status: Status::ok(v),
                        }),
                    );
                }
                status(fid::START_SCAN, Status::SL_OK)
            }
            Command::LookupEui64ByNodeId { node_id } => match mesh.ieee_of(node_id) {
                Some(ieee) => Response::LookupEui64ByNodeId { status: ok, ieee },
                None => Response::LookupEui64ByNodeId {
                    status: Status::fail(v),
                    ieee: Ieee::ZERO,
                },
            },
            Command::SetRadioPower { .. } => status(fid::SET_RADIO_POWER, ok),
            Command::Echo { data } => Response::Echo { data },
            Command::Nop => Response::Empty { frame_id: fid::NOP },
        }
    }
}

/// Serve one host connection over `io` until it ends.
pub(crate) async fn serve<T>(sim: &Simulator, io: T) -> Result<(), SimError>
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let (link, mut link_events) = Link::spawn(io, Role::Ncp, AshConfig::default());
    let (action_tx, mut actions) = mpsc::unbounded_channel::<(Duration, Scheduled)>();
    let (due_tx, mut due) = mpsc::unbounded_channel::<Scheduled>();
    lock(&sim.state).mesh.sink = Some(action_tx.clone());
    let mut extended = false;
    let mut callback_seq: u8 = 0;
    let result = 'serve: loop {
        let callbacks: Vec<Callback> = tokio::select! {
            event = link_events.recv() => match event {
                None => break Err(SimError::Closed("link task stopped".into())),
                Some(LinkEvent::Closed(why)) => break Err(SimError::Closed(why)),
                Some(LinkEvent::Ash(AshEvent::Connected { .. })) => {
                    extended = false;
                    let mut s = lock(&sim.state);
                    let s = &mut *s;
                    if let Some(front) = s.front.ezsp() {
                        front.reset(&mut s.mesh);
                    }
                    continue;
                }
                Some(LinkEvent::Ash(AshEvent::Down(reason))) => {
                    tracing::debug!(?reason, "sim: link down");
                    continue;
                }
                Some(LinkEvent::Ash(AshEvent::Data(data))) => {
                    let format = if extended { Format::Extended } else { frame::detect_command_format(&data) };
                    let Ok(header) = frame::parse(&data, format) else { continue };
                    let response = {
                        let mut s = lock(&sim.state);
                        let s = &mut *s;
                        let Some(front) = s.front.ezsp() else { continue };
                        let v = ProtocolVersion(front.protocol);
                        let response = match Command::decode(v, header.frame_id, header.params) {
                            Ok(command) => {
                                tracing::trace!(?command, "sim: command");
                                if format == Format::Extended && matches!(command, Command::Version { .. }) {
                                    extended = true;
                                }
                                front.command(&mut s.mesh, v, command)
                            }
                            Err(e) => {
                                tracing::debug!(frame_id = header.frame_id, error = %e, "sim: invalid command");
                                Response::InvalidCommand { reason: 0x31 }
                            }
                        };
                        frame::encode(format, header.seq, CONTROL_RESPONSE, response.frame_id(), &response.encode(v))
                    };
                    if link.send(response).await.is_err() {
                        break Err(SimError::Closed("send failed".into()));
                    }
                    continue;
                }
            },
            Some((delay, action)) = actions.recv() => {
                let due_tx = due_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let _ = due_tx.send(action);
                });
                continue;
            }
            Some(action) = due.recv() => {
                let mut s = lock(&sim.state);
                let s = &mut *s;
                match action {
                    Scheduled::Join(ieee) => {
                        s.mesh.join(ieee);
                        continue;
                    }
                    Scheduled::Remove(ieee) => {
                        s.mesh.leave_device(ieee);
                        continue;
                    }
                    Scheduled::Ezsp(cb) => vec![cb],
                    Scheduled::Mesh(event) => match s.front.ezsp() {
                        Some(front) => front.translate(event),
                        None => continue,
                    },
                    Scheduled::Znp(_) | Scheduled::Deconz(_) => continue,
                }
            }
        };
        if !extended {
            continue;
        }
        let v = match lock(&sim.state).front.ezsp() {
            Some(front) => ProtocolVersion(front.protocol),
            None => continue,
        };
        for cb in callbacks {
            callback_seq = callback_seq.wrapping_add(1);
            let bytes = frame::encode(
                Format::Extended,
                callback_seq,
                CONTROL_RESPONSE | CONTROL_ASYNC_CALLBACK,
                cb.frame_id(),
                &cb.encode(v),
            );
            if link.send(bytes).await.is_err() {
                break 'serve Err(SimError::Closed("send failed".into()));
            }
        }
    };
    // Only this connection's channel: a newer connection may have replaced
    // it already.
    let mut state = lock(&sim.state);
    if state
        .mesh
        .sink
        .as_ref()
        .is_some_and(|a| a.same_channel(&action_tx))
    {
        state.mesh.sink = None;
    }
    result
}
