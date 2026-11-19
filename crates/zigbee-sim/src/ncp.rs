//! The simulated network co-processor.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use zigbee_ash::{Config as AshConfig, Event as AshEvent, Link, LinkEvent, Role};
use zigbee_ezsp::frame::{self, Format, CONTROL_ASYNC_CALLBACK, CONTROL_RESPONSE};
use zigbee_ezsp::ids::{self, frame as fid, key_type, mfg_token, tc_decision, value};
use zigbee_ezsp::types::{
    ApsKeyMetadata, NetworkKeyInfo, NetworkParameters, SecManContext, StackVersion,
};
use zigbee_ezsp::{ApsFrame, Callback, Command, ProtocolVersion, Response, RxPacketInfo, Status};
use zigbee_types::{Ieee, Key, NodeId};
use zigbee_zcl::Value;

use crate::device::{DeviceState, Outgoing, SimDevice};

/// Simulator settings.
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// The coordinator's factory IEEE address.
    pub ieee: Ieee,
    /// The EZSP protocol version the NCP speaks (13 for EmberZNet 7.4,
    /// 16 to 19 for 8.x).
    pub protocol: u8,
    /// The EmberZNet version it reports.
    pub stack_version: StackVersion,
    /// The simulated devices.
    pub devices: Vec<SimDevice>,
    /// Delay before a device in pairing mode joins an open network.
    pub join_delay: Duration,
    /// Delay of every device reply.
    pub reply_delay: Duration,
    /// Key table entries.
    pub key_table_size: u16,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            ieee: Ieee(0x00_0D_6F_FF_FE_00_00_01),
            protocol: 13,
            stack_version: StackVersion {
                build: 380,
                major: 7,
                minor: 4,
                patch: 4,
                special: 0,
                kind: 0xAA,
            },
            devices: Vec::new(),
            join_delay: Duration::from_millis(200),
            reply_delay: Duration::from_millis(20),
            key_table_size: 12,
        }
    }
}

/// The network the NCP has stored (its "tokens").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimNetwork {
    /// Parameters it was formed with.
    pub parameters: NetworkParameters,
    /// Network key.
    pub network_key: Key,
    /// Key sequence number.
    pub key_sequence: u8,
    /// Trust center root (hashed link key) key.
    pub tc_root: Key,
    /// Outgoing network frame counter.
    pub frame_counter: u32,
}

/// A point-in-time view for tests.
#[derive(Debug, Clone)]
pub struct SimSnapshot {
    /// The stored network, if any.
    pub network: Option<SimNetwork>,
    /// Joined devices and their network addresses.
    pub joined: Vec<(Ieee, NodeId)>,
    /// Bindings: (device, endpoint, cluster).
    pub bindings: Vec<(Ieee, u8, u16)>,
    /// Reporting configurations: (device, endpoint, cluster, attribute).
    pub reporting: Vec<(Ieee, u8, u16, u16)>,
    /// Joining is open.
    pub permit_join: bool,
    /// The IEEE address the NCP reports (factory or written token).
    pub ieee: Ieee,
    /// Key table entries.
    pub key_table: Vec<(Ieee, Key)>,
    /// IAS-enrolled devices.
    pub ias_enrolled: Vec<Ieee>,
}

#[derive(Debug)]
enum Action {
    Callback(Callback),
    Join(Ieee),
}

struct Joined {
    nwk: NodeId,
    state: DeviceState,
}

struct NcpState {
    config: SimConfig,
    devices: BTreeMap<Ieee, SimDevice>,
    joined: BTreeMap<Ieee, Joined>,
    network: Option<SimNetwork>,
    up: bool,
    security: Option<zigbee_ezsp::types::InitialSecurityState>,
    tc_policy: u8,
    permit_until: Option<Instant>,
    transient: HashMap<Ieee, Key>,
    key_table: BTreeMap<u16, (Ieee, Key)>,
    custom_eui: Option<Ieee>,
    custom_eui_pending: Option<Ieee>,
    pending_frame_counter: Option<u32>,
    endpoints: Vec<u8>,
    next_nwk: NodeId,
    aps_seq: u8,
    actions: Option<mpsc::UnboundedSender<(Duration, Action)>>,
}

impl NcpState {
    fn ieee(&self) -> Ieee {
        self.custom_eui.unwrap_or(self.config.ieee)
    }

    fn schedule(&self, delay: Duration, action: Action) {
        if let Some(tx) = &self.actions {
            let _ = tx.send((delay, action));
        }
    }

    fn deliver(&mut self, from: Ieee, out: Outgoing, delay: Duration) {
        let Some(nwk) = self.joined.get(&from).map(|j| j.nwk) else {
            return;
        };
        let (lqi, rssi) = self
            .devices
            .get(&from)
            .map(|d| (d.lqi, d.rssi))
            .unwrap_or((200, -50));
        if let Some(n) = self.network.as_mut() {
            n.frame_counter = n.frame_counter.wrapping_add(1);
        }
        self.aps_seq = self.aps_seq.wrapping_add(1);
        let aps = ApsFrame {
            profile_id: out.profile,
            cluster_id: out.cluster,
            source_endpoint: out.source_endpoint,
            destination_endpoint: out.destination_endpoint,
            options: 0,
            group_id: 0,
            sequence: self.aps_seq,
        };
        let v = ProtocolVersion(self.config.protocol);
        let info = RxPacketInfo {
            sender: nwk,
            sender_ieee: v.sl_status().then_some(from),
            binding_index: 0xFF,
            address_index: 0xFF,
            last_hop_lqi: lqi,
            last_hop_rssi: rssi,
            last_hop_timestamp: 0,
        };
        self.schedule(
            delay,
            Action::Callback(Callback::IncomingMessage {
                kind: ids::incoming::UNICAST,
                aps,
                info,
                payload: out.payload,
            }),
        );
    }

    fn permit_open(&self) -> bool {
        self.permit_until.is_some_and(|t| Instant::now() < t)
    }

    /// Whether a device's join would be accepted now.
    fn may_join(&self, device: &SimDevice) -> bool {
        if !self.up || !self.permit_open() || self.tc_policy & tc_decision::ALLOW_JOINS == 0 {
            return false;
        }
        match device.install_code_key {
            Some(key) => self.transient.get(&device.ieee) == Some(&key),
            None => {
                self.tc_policy & tc_decision::JOINS_USE_INSTALL_CODE_KEY == 0
                    && self.transient.get(&Ieee::ZERO) == Some(&Key::ZIGBEE_ALLIANCE_09)
            }
        }
    }

    fn schedule_joins(&self) {
        for d in self.devices.values() {
            if d.pairing && !self.joined.contains_key(&d.ieee) {
                self.schedule(self.config.join_delay, Action::Join(d.ieee));
            }
        }
    }

    fn join(&mut self, ieee: Ieee) {
        let Some(device) = self.devices.get(&ieee).cloned() else {
            return;
        };
        if self.joined.contains_key(&ieee) || !self.may_join(&device) {
            return;
        }
        let nwk = self.next_nwk;
        self.next_nwk = self.next_nwk.wrapping_add(0x1111);
        self.joined.insert(
            ieee,
            Joined {
                nwk,
                state: DeviceState::default(),
            },
        );
        tracing::info!(%ieee, nwk = format!("{nwk:#06x}"), "sim: device joined");
        let step = Duration::from_millis(10);
        self.schedule(
            Duration::ZERO,
            Action::Callback(Callback::TrustCenterJoin {
                node_id: nwk,
                ieee,
                status: ids::device_update::UNSECURED_JOIN,
                decision: ids::join_decision::USE_PRECONFIGURED_KEY,
                parent: 0,
            }),
        );
        if !device.router {
            self.schedule(
                step,
                Action::Callback(Callback::ChildJoin {
                    index: 0,
                    joining: true,
                    node_id: nwk,
                    ieee,
                    node_type: if device.sleepy {
                        ids::node_type::SLEEPY_END_DEVICE
                    } else {
                        ids::node_type::END_DEVICE
                    },
                }),
            );
        }
        if !device.legacy {
            self.schedule(
                step * 2,
                Action::Callback(Callback::ZigbeeKeyEstablishment {
                    partner: ieee,
                    status: ids::key_status::TC_RESPONDED_TO_KEY_REQUEST,
                }),
            );
        }
        let announce = device.announce(nwk);
        self.deliver(ieee, announce, step * 3);
    }

    fn leave_device(&mut self, ieee: Ieee) {
        if let Some(j) = self.joined.remove(&ieee) {
            self.schedule(
                Duration::from_millis(5),
                Action::Callback(Callback::TrustCenterJoin {
                    node_id: j.nwk,
                    ieee,
                    status: ids::device_update::DEVICE_LEFT,
                    decision: ids::join_decision::NO_ACTION,
                    parent: 0,
                }),
            );
        }
    }

    fn handle_unicast(&mut self, destination: NodeId, aps: ApsFrame, payload: &[u8]) -> bool {
        let Some(ieee) = self
            .joined
            .iter()
            .find(|(_, j)| j.nwk == destination)
            .map(|(i, _)| *i)
        else {
            return false;
        };
        let coordinator = self.ieee();
        let delay = self.config.reply_delay;
        let Some(mut device) = self.devices.get(&ieee).cloned() else {
            return false;
        };
        let Some(joined) = self.joined.get_mut(&ieee) else {
            return false;
        };
        let reaction = if aps.profile_id == zigbee_zdo::PROFILE_ID {
            device.handle_zdo(
                destination,
                &mut joined.state,
                aps.cluster_id,
                payload,
                coordinator,
            )
        } else {
            device.handle_zcl(
                &mut joined.state,
                aps.destination_endpoint,
                aps.cluster_id,
                aps.source_endpoint,
                payload,
            )
        };
        self.devices.insert(ieee, device);
        for reply in reaction.replies {
            self.deliver(ieee, reply, delay);
        }
        if reaction.leave {
            self.leave_device(ieee);
        }
        true
    }

    fn snapshot(&self) -> SimSnapshot {
        let mut bindings = Vec::new();
        let mut reporting = Vec::new();
        for (ieee, j) in &self.joined {
            bindings.extend(
                j.state
                    .bindings
                    .iter()
                    .map(|b| (*ieee, b.source_endpoint, b.cluster)),
            );
            reporting.extend(
                j.state
                    .reporting
                    .keys()
                    .map(|(e, c, a)| (*ieee, *e, *c, *a)),
            );
        }
        reporting.sort_unstable();
        SimSnapshot {
            network: self.network.clone(),
            joined: self.joined.iter().map(|(i, j)| (*i, j.nwk)).collect(),
            bindings,
            reporting,
            permit_join: self.permit_open(),
            ieee: self.ieee(),
            key_table: self.key_table.values().copied().collect(),
            ias_enrolled: self
                .joined
                .iter()
                .filter(|(_, j)| j.state.ias_enrolled)
                .map(|(i, _)| *i)
                .collect(),
        }
    }

    /// The NCP reset (RST): the stack restarts, the stored network stays.
    fn reset(&mut self) {
        self.up = false;
        self.endpoints.clear();
        self.security = None;
        self.tc_policy = 0;
        self.permit_until = None;
        self.transient.clear();
        if let Some(eui) = self.custom_eui_pending.take() {
            self.custom_eui = Some(eui);
        }
    }

    fn command(&mut self, v: ProtocolVersion, command: Command) -> Response {
        let ok = Status::ok(v);
        let status = |frame_id: u16, status: Status| Response::Status { frame_id, status };
        match command {
            Command::Version { desired } => Response::Version {
                protocol_version: if desired == self.config.protocol {
                    desired
                } else {
                    self.config.protocol
                },
                stack_type: ids::STACK_TYPE_MESH,
                stack_version: (u16::from(self.config.stack_version.major) << 12)
                    | (u16::from(self.config.stack_version.minor & 0xF) << 8)
                    | (u16::from(self.config.stack_version.patch & 0xF) << 4)
                    | u16::from(self.config.stack_version.special & 0xF),
            },
            Command::GetConfigurationValue { config_id } => Response::GetConfigurationValue {
                status: ok,
                value: if config_id == ids::config::KEY_TABLE_SIZE {
                    self.config.key_table_size
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
                    self.tc_policy = decision;
                }
                status(fid::SET_POLICY, ok)
            }
            Command::GetValue { value_id } => match value_id {
                value::VERSION_INFO => Response::GetValue {
                    status: ok,
                    value: self.config.stack_version.to_bytes(),
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
                if self.network.is_some() {
                    self.up = true;
                    self.schedule(
                        Duration::from_millis(5),
                        Action::Callback(Callback::StackStatus {
                            status: Status::network_up(v),
                        }),
                    );
                    status(fid::NETWORK_INIT, ok)
                } else {
                    status(fid::NETWORK_INIT, Status::not_joined(v))
                }
            }
            Command::NetworkState => Response::NetworkState {
                state: if self.up {
                    ids::network_status::JOINED_NETWORK
                } else {
                    ids::network_status::NO_NETWORK
                },
            },
            Command::FormNetwork { parameters } => {
                let Some(security) = self.security else {
                    return status(fid::FORM_NETWORK, Status::invalid_call(v));
                };
                if self.network.is_some() {
                    return status(fid::FORM_NETWORK, Status::invalid_call(v));
                }
                self.network = Some(SimNetwork {
                    parameters,
                    network_key: security.network_key,
                    key_sequence: security.network_key_sequence,
                    tc_root: security.preconfigured_key,
                    frame_counter: self.pending_frame_counter.take().unwrap_or(0),
                });
                self.up = true;
                self.schedule(
                    Duration::from_millis(5),
                    Action::Callback(Callback::StackStatus {
                        status: Status::network_up(v),
                    }),
                );
                status(fid::FORM_NETWORK, ok)
            }
            Command::LeaveNetwork { .. } => {
                if self.network.take().is_none() {
                    return status(fid::LEAVE_NETWORK, Status::invalid_call(v));
                }
                self.up = false;
                self.joined.clear();
                self.schedule(
                    Duration::from_millis(5),
                    Action::Callback(Callback::StackStatus {
                        status: Status::network_down(v),
                    }),
                );
                status(fid::LEAVE_NETWORK, ok)
            }
            Command::PermitJoining { duration } => {
                if !self.up {
                    return status(fid::PERMIT_JOINING, Status::network_down(v));
                }
                if duration == 0 {
                    self.permit_until = None;
                    self.schedule(
                        Duration::ZERO,
                        Action::Callback(Callback::StackStatus {
                            status: Status::network_closed(v),
                        }),
                    );
                } else {
                    self.permit_until =
                        Some(Instant::now() + Duration::from_secs(u64::from(duration)));
                    self.schedule(
                        Duration::ZERO,
                        Action::Callback(Callback::StackStatus {
                            status: Status::network_opened(v),
                        }),
                    );
                    self.schedule_joins();
                }
                status(fid::PERMIT_JOINING, ok)
            }
            Command::GetNetworkParameters => match &self.network {
                Some(n) if self.up => Response::GetNetworkParameters {
                    status: ok,
                    node_type: ids::node_type::COORDINATOR,
                    parameters: n.parameters,
                },
                _ => Response::GetNetworkParameters {
                    status: Status::not_joined(v),
                    node_type: 0,
                    parameters: NetworkParameters::default(),
                },
            },
            Command::GetEui64 => Response::GetEui64 { ieee: self.ieee() },
            Command::GetNodeId => Response::GetNodeId {
                node_id: if self.up { 0x0000 } else { 0xFFFE },
            },
            Command::SetInitialSecurityState { state } => {
                self.security = Some(state);
                status(fid::SET_INITIAL_SECURITY_STATE, ok)
            }
            Command::ExportKey { context } => {
                let key = match (context.core_key_type, &self.network) {
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
            Command::GetNetworkKeyInfo => match &self.network {
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
                if u16::from(index) >= self.config.key_table_size {
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
                self.transient.insert(ieee, key);
                if self.permit_open() {
                    self.schedule_joins();
                }
                status(fid::IMPORT_TRANSIENT_KEY, Status::SL_OK)
            }
            Command::ClearTransientLinkKeys => {
                self.transient.clear();
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
                if !self.up {
                    return Response::Sent {
                        frame_id: fid::SEND_UNICAST,
                        status: Status::network_down(v),
                        aps_sequence: 0,
                    };
                }
                let delivered = self.handle_unicast(destination, aps, &payload);
                self.aps_seq = self.aps_seq.wrapping_add(1);
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
                self.schedule(Duration::from_millis(2), Action::Callback(sent));
                Response::Sent {
                    frame_id: fid::SEND_UNICAST,
                    status: ok,
                    aps_sequence: self.aps_seq,
                }
            }
            Command::SendBroadcast { .. } => {
                self.aps_seq = self.aps_seq.wrapping_add(1);
                Response::Sent {
                    frame_id: fid::SEND_BROADCAST,
                    status: if self.up { ok } else { Status::network_down(v) },
                    aps_sequence: self.aps_seq,
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
                if scan_type == ids::scan_type::ENERGY {
                    let mut last = 11;
                    for channel in 11..=26u8 {
                        if channel_mask & (1 << channel) != 0 {
                            let max_rssi = -95 + ((channel * 7) % 30) as i8;
                            self.schedule(
                                Duration::from_millis(u64::from(channel)),
                                Action::Callback(Callback::EnergyScanResult { channel, max_rssi }),
                            );
                            last = channel;
                        }
                    }
                    self.schedule(
                        Duration::from_millis(40),
                        Action::Callback(Callback::ScanComplete {
                            channel: last,
                            status: Status::ok(v),
                        }),
                    );
                }
                status(fid::START_SCAN, Status::SL_OK)
            }
            Command::LookupEui64ByNodeId { node_id } => {
                match self.joined.iter().find(|(_, j)| j.nwk == node_id) {
                    Some((ieee, _)) => Response::LookupEui64ByNodeId {
                        status: ok,
                        ieee: *ieee,
                    },
                    None => Response::LookupEui64ByNodeId {
                        status: Status::fail(v),
                        ieee: Ieee::ZERO,
                    },
                }
            }
            Command::SetRadioPower { .. } => status(fid::SET_RADIO_POWER, ok),
            Command::Echo { data } => Response::Echo { data },
            Command::Nop => Response::Empty { frame_id: fid::NOP },
        }
    }
}

/// An EZSP NCP simulator. State (the stored network, joined devices,
/// bindings) survives host reconnects, as an NCP's tokens survive resets.
#[derive(Clone)]
pub struct Simulator {
    state: Arc<Mutex<NcpState>>,
}

impl std::fmt::Debug for Simulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Simulator").finish_non_exhaustive()
    }
}

/// The simulator could not run.
#[derive(Debug, thiserror::Error)]
pub enum SimError {
    /// The host's link ended.
    #[error("host link closed: {0}")]
    Closed(String),
}

fn lock(m: &Mutex<NcpState>) -> MutexGuard<'_, NcpState> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Simulator {
    /// A simulator with `config`'s devices, all powered on.
    pub fn new(config: SimConfig) -> Simulator {
        let devices = config.devices.iter().map(|d| (d.ieee, d.clone())).collect();
        Simulator {
            state: Arc::new(Mutex::new(NcpState {
                config,
                devices,
                joined: BTreeMap::new(),
                network: None,
                up: false,
                security: None,
                tc_policy: 0,
                permit_until: None,
                transient: HashMap::new(),
                key_table: BTreeMap::new(),
                custom_eui: None,
                custom_eui_pending: None,
                pending_frame_counter: None,
                endpoints: Vec::new(),
                next_nwk: 0x1A2B,
                aps_seq: 0,
                actions: None,
            })),
        }
    }

    /// Everything the simulator holds, for assertions.
    pub fn snapshot(&self) -> SimSnapshot {
        lock(&self.state).snapshot()
    }

    /// Set a device attribute, reporting it if the device was configured to.
    /// Returns how many reports were sent.
    pub fn set_attribute(
        &self,
        ieee: Ieee,
        endpoint: u8,
        cluster: u16,
        attribute: u16,
        value: Value,
    ) -> usize {
        let mut s = lock(&self.state);
        let Some(mut device) = s.devices.get(&ieee).cloned() else {
            return 0;
        };
        let reports = match s.joined.get_mut(&ieee) {
            Some(j) => device.set_attribute(&mut j.state, endpoint, cluster, attribute, value),
            None => {
                device.set_attribute(
                    &mut DeviceState::default(),
                    endpoint,
                    cluster,
                    attribute,
                    value,
                );
                Vec::new()
            }
        };
        s.devices.insert(ieee, device);
        let n = reports.len();
        for r in reports {
            s.deliver(ieee, r, Duration::ZERO);
        }
        n
    }

    /// A device sends a cluster command frame (a button press, an alarm).
    pub fn send_from(&self, ieee: Ieee, out: Outgoing) {
        lock(&self.state).deliver(ieee, out, Duration::ZERO);
    }

    /// Add a device (it joins the next time the network opens).
    pub fn add_device(&self, device: SimDevice) {
        let mut s = lock(&self.state);
        let ieee = device.ieee;
        s.devices.insert(ieee, device);
        if s.permit_open() {
            let delay = s.config.join_delay;
            s.schedule(delay, Action::Join(ieee));
        }
    }

    /// A device leaves the network by itself.
    pub fn leave(&self, ieee: Ieee) {
        lock(&self.state).leave_device(ieee);
    }

    /// Serve one host connection over `io` until it ends.
    pub async fn serve<T>(&self, io: T) -> Result<(), SimError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (link, mut link_events) = Link::spawn(io, Role::Ncp, AshConfig::default());
        let (action_tx, mut actions) = mpsc::unbounded_channel::<(Duration, Action)>();
        let (due_tx, mut due) = mpsc::unbounded_channel::<Action>();
        lock(&self.state).actions = Some(action_tx.clone());
        let mut extended = false;
        let mut callback_seq: u8 = 0;
        let result = loop {
            tokio::select! {
                event = link_events.recv() => match event {
                    None => break Err(SimError::Closed("link task stopped".into())),
                    Some(LinkEvent::Closed(why)) => break Err(SimError::Closed(why)),
                    Some(LinkEvent::Ash(AshEvent::Connected { .. })) => {
                        extended = false;
                        lock(&self.state).reset();
                    }
                    Some(LinkEvent::Ash(AshEvent::Down(reason))) => tracing::debug!(?reason, "sim: link down"),
                    Some(LinkEvent::Ash(AshEvent::Data(data))) => {
                        let format = if extended { Format::Extended } else { frame::detect_command_format(&data) };
                        let Ok(header) = frame::parse(&data, format) else { continue };
                        let v = ProtocolVersion(lock(&self.state).config.protocol);
                        let response = match Command::decode(v, header.frame_id, header.params) {
                            Ok(command) => {
                                tracing::trace!(?command, "sim: command");
                                if format == Format::Extended && matches!(command, Command::Version { .. }) {
                                    extended = true;
                                }
                                lock(&self.state).command(v, command)
                            }
                            Err(e) => {
                                tracing::debug!(frame_id = header.frame_id, error = %e, "sim: invalid command");
                                Response::InvalidCommand { reason: 0x31 }
                            }
                        };
                        let bytes = frame::encode(format, header.seq, CONTROL_RESPONSE, response.frame_id(), &response.encode(v));
                        if link.send(bytes).await.is_err() {
                            break Err(SimError::Closed("send failed".into()));
                        }
                    }
                },
                Some((delay, action)) = actions.recv() => {
                    let due_tx = due_tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(delay).await;
                        let _ = due_tx.send(action);
                    });
                }
                Some(action) = due.recv() => match action {
                    Action::Join(ieee) => lock(&self.state).join(ieee),
                    Action::Callback(cb) => {
                        if !extended {
                            continue;
                        }
                        let v = ProtocolVersion(lock(&self.state).config.protocol);
                        callback_seq = callback_seq.wrapping_add(1);
                        let bytes = frame::encode(Format::Extended, callback_seq, CONTROL_RESPONSE | CONTROL_ASYNC_CALLBACK, cb.frame_id(), &cb.encode(v));
                        if link.send(bytes).await.is_err() {
                            break Err(SimError::Closed("send failed".into()));
                        }
                    }
                },
            }
        };
        // Only this connection's channel: a newer connection may have
        // replaced it already.
        let mut state = lock(&self.state);
        if state
            .actions
            .as_ref()
            .is_some_and(|a| a.same_channel(&action_tx))
        {
            state.actions = None;
        }
        result
    }
}
