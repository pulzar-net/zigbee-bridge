//! Simulated Zigbee devices: they answer ZDO and ZCL like real ones and
//! report attributes the way they were configured to.

use std::collections::{BTreeMap, HashMap};

use zigbee_types::{Ieee, Key, NodeId};
use zigbee_zcl::foundation::{self, Global, ReadRecord, ReportingConfig, ReportingStatus};
use zigbee_zcl::{attr, cluster_id, status, DataType, Direction, Frame, FrameType, Value};
use zigbee_zdo::{Binding, Body, Message, NodeDescriptor, SimpleDescriptor};

/// One endpoint of a simulated device.
#[derive(Debug, Clone)]
pub struct SimEndpoint {
    /// Endpoint number.
    pub endpoint: u8,
    /// Application profile.
    pub profile_id: u16,
    /// Device id.
    pub device_id: u16,
    /// Server clusters and their attribute values.
    pub servers: BTreeMap<u16, BTreeMap<u16, Value>>,
    /// Client clusters.
    pub clients: Vec<u16>,
}

/// A simulated device.
#[derive(Debug, Clone)]
pub struct SimDevice {
    /// IEEE address.
    pub ieee: Ieee,
    /// A router (true) or an end device.
    pub router: bool,
    /// The receiver is off when idle (a sleepy end device).
    pub sleepy: bool,
    /// Does not request a unique link key after joining (pre-Zigbee 3.0).
    pub legacy: bool,
    /// Joins only with this install code key.
    pub install_code_key: Option<Key>,
    /// Manufacturer code in the node descriptor.
    pub manufacturer_code: u16,
    /// Endpoints.
    pub endpoints: Vec<SimEndpoint>,
    /// Link quality the coordinator hears it at.
    pub lqi: u8,
    /// Signal strength the coordinator hears it at.
    pub rssi: i8,
    /// Join when the network opens (a powered-on device in pairing mode).
    pub pairing: bool,
}

fn basic(manufacturer: &str, model: &str, power_source: u8) -> BTreeMap<u16, Value> {
    BTreeMap::from([
        (attr::basic::ZCL_VERSION, Value::u8(8)),
        (attr::basic::APPLICATION_VERSION, Value::u8(1)),
        (attr::basic::STACK_VERSION, Value::u8(2)),
        (attr::basic::HW_VERSION, Value::u8(1)),
        (attr::basic::MANUFACTURER_NAME, Value::string(manufacturer)),
        (attr::basic::MODEL_IDENTIFIER, Value::string(model)),
        (attr::basic::DATE_CODE, Value::string("20261119")),
        (attr::basic::POWER_SOURCE, Value::enum8(power_source)),
        (attr::basic::SW_BUILD_ID, Value::string("1.0.0")),
    ])
}

/// The manufacturer name simulated devices report.
pub const SIM_MANUFACTURER: &str = "zigbee-bridge-sim";

impl SimDevice {
    /// A battery temperature and humidity sensor (sleepy end device).
    pub fn temperature_humidity_sensor(ieee: Ieee) -> SimDevice {
        let servers = BTreeMap::from([
            (cluster_id::BASIC, basic(SIM_MANUFACTURER, "SIM-TH1", 0x03)),
            (
                cluster_id::POWER_CONFIGURATION,
                BTreeMap::from([
                    (attr::power_configuration::BATTERY_VOLTAGE, Value::u8(30)),
                    (
                        attr::power_configuration::BATTERY_PERCENTAGE_REMAINING,
                        Value::u8(200),
                    ),
                ]),
            ),
            (
                cluster_id::IDENTIFY,
                BTreeMap::from([(attr::identify::IDENTIFY_TIME, Value::u16(0))]),
            ),
            (
                cluster_id::TEMPERATURE_MEASUREMENT,
                BTreeMap::from([
                    (
                        attr::temperature_measurement::MEASURED_VALUE,
                        Value::i16(2000),
                    ),
                    (
                        attr::temperature_measurement::MIN_MEASURED_VALUE,
                        Value::i16(-4000),
                    ),
                    (
                        attr::temperature_measurement::MAX_MEASURED_VALUE,
                        Value::i16(12500),
                    ),
                ]),
            ),
            (
                cluster_id::RELATIVE_HUMIDITY_MEASUREMENT,
                BTreeMap::from([(
                    attr::relative_humidity_measurement::MEASURED_VALUE,
                    Value::u16(4500),
                )]),
            ),
        ]);
        SimDevice {
            ieee,
            router: false,
            sleepy: true,
            legacy: false,
            install_code_key: None,
            manufacturer_code: 0x1234,
            endpoints: vec![SimEndpoint {
                endpoint: 1,
                profile_id: 0x0104,
                device_id: 0x0302,
                servers,
                clients: vec![cluster_id::OTA_UPGRADE],
            }],
            lqi: 180,
            rssi: -58,
            pairing: true,
        }
    }

    /// A mains-powered on/off plug with power metering (router).
    pub fn plug(ieee: Ieee) -> SimDevice {
        let servers = BTreeMap::from([
            (
                cluster_id::BASIC,
                basic(SIM_MANUFACTURER, "SIM-PLUG1", 0x01),
            ),
            (
                cluster_id::IDENTIFY,
                BTreeMap::from([(attr::identify::IDENTIFY_TIME, Value::u16(0))]),
            ),
            (
                cluster_id::GROUPS,
                BTreeMap::from([(attr::groups::NAME_SUPPORT, Value::map8(0))]),
            ),
            (
                cluster_id::ON_OFF,
                BTreeMap::from([(attr::on_off::ON_OFF, Value::bool(false))]),
            ),
            (
                cluster_id::ELECTRICAL_MEASUREMENT,
                BTreeMap::from([
                    (
                        attr::electrical_measurement::ACTIVE_POWER,
                        Value::Int(DataType::Int16, 0),
                    ),
                    (attr::electrical_measurement::RMS_VOLTAGE, Value::u16(230)),
                    (
                        attr::electrical_measurement::AC_POWER_MULTIPLIER,
                        Value::u16(1),
                    ),
                    (
                        attr::electrical_measurement::AC_POWER_DIVISOR,
                        Value::u16(1),
                    ),
                ]),
            ),
        ]);
        SimDevice {
            ieee,
            router: true,
            sleepy: false,
            legacy: false,
            install_code_key: None,
            manufacturer_code: 0x1234,
            endpoints: vec![SimEndpoint {
                endpoint: 1,
                profile_id: 0x0104,
                device_id: 0x0051,
                servers,
                clients: vec![cluster_id::OTA_UPGRADE],
            }],
            lqi: 230,
            rssi: -40,
            pairing: true,
        }
    }

    /// A door contact sensor using an IAS zone (sleepy end device).
    pub fn contact_sensor(ieee: Ieee) -> SimDevice {
        let servers = BTreeMap::from([
            (
                cluster_id::BASIC,
                basic(SIM_MANUFACTURER, "SIM-DOOR1", 0x03),
            ),
            (
                cluster_id::POWER_CONFIGURATION,
                BTreeMap::from([(
                    attr::power_configuration::BATTERY_PERCENTAGE_REMAINING,
                    Value::u8(180),
                )]),
            ),
            (
                cluster_id::IAS_ZONE,
                BTreeMap::from([
                    (attr::ias_zone::ZONE_STATE, Value::enum8(0)),
                    (attr::ias_zone::ZONE_TYPE, Value::enum16(0x0015)),
                    (attr::ias_zone::ZONE_STATUS, Value::map16(0)),
                    (attr::ias_zone::IAS_CIE_ADDRESS, Value::Ieee(Ieee::ZERO)),
                    (attr::ias_zone::ZONE_ID, Value::u8(0xFF)),
                ]),
            ),
        ]);
        SimDevice {
            ieee,
            router: false,
            sleepy: true,
            legacy: false,
            install_code_key: None,
            manufacturer_code: 0x1234,
            endpoints: vec![SimEndpoint {
                endpoint: 1,
                profile_id: 0x0104,
                device_id: 0x0402,
                servers,
                clients: vec![],
            }],
            lqi: 150,
            rssi: -70,
            pairing: true,
        }
    }

    fn node_descriptor(&self) -> NodeDescriptor {
        let (logical, mac) = if self.router {
            (1, 0x8E)
        } else if self.sleepy {
            (2, 0x80)
        } else {
            (2, 0x88)
        };
        NodeDescriptor {
            byte0: logical,
            byte1: 0x40,
            mac_capabilities: mac,
            manufacturer_code: self.manufacturer_code,
            max_buffer_size: 82,
            max_incoming_transfer: 82,
            server_mask: 22 << 9,
            max_outgoing_transfer: 82,
            descriptor_capability: 0,
        }
    }

    fn simple_descriptor(&self, endpoint: u8) -> Option<SimpleDescriptor> {
        self.endpoints
            .iter()
            .find(|e| e.endpoint == endpoint)
            .map(|e| SimpleDescriptor {
                endpoint: e.endpoint,
                profile_id: e.profile_id,
                device_id: e.device_id,
                device_version: 1,
                input_clusters: e.servers.keys().copied().collect(),
                output_clusters: e.clients.clone(),
            })
    }
}

/// What a joined device remembers.
#[derive(Debug, Clone, Default)]
pub struct DeviceState {
    /// Its bindings.
    pub bindings: Vec<Binding>,
    /// Its reporting configuration, by (endpoint, cluster, attribute).
    pub reporting: HashMap<(u8, u16, u16), ReportingConfig>,
    /// ZCL sequence for frames it originates.
    pub seq: u8,
    /// Enrolled in an IAS zone.
    pub ias_enrolled: bool,
}

/// A frame a device sends to the coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// Profile.
    pub profile: u16,
    /// Cluster.
    pub cluster: u16,
    /// Source endpoint.
    pub source_endpoint: u8,
    /// Destination endpoint.
    pub destination_endpoint: u8,
    /// APS payload.
    pub payload: Vec<u8>,
}

/// What a device does with a frame from the coordinator.
#[derive(Debug, Default)]
pub struct Reaction {
    /// Frames it sends back.
    pub replies: Vec<Outgoing>,
    /// It leaves the network.
    pub leave: bool,
}

impl SimDevice {
    /// Handle a ZDO request addressed to this device.
    pub fn handle_zdo(
        &self,
        nwk: NodeId,
        state: &mut DeviceState,
        cluster: u16,
        payload: &[u8],
        coordinator: Ieee,
    ) -> Reaction {
        let mut reaction = Reaction::default();
        let Ok(request) = Message::parse(cluster, payload) else {
            return reaction;
        };
        let body = match request.body {
            Body::NodeDescReq { .. } => Body::NodeDescRsp {
                status: 0,
                nwk,
                descriptor: Some(self.node_descriptor()),
            },
            Body::ActiveEpReq { .. } => Body::ActiveEpRsp {
                status: 0,
                nwk,
                endpoints: self.endpoints.iter().map(|e| e.endpoint).collect(),
            },
            Body::SimpleDescReq { endpoint, .. } => match self.simple_descriptor(endpoint) {
                Some(d) => Body::SimpleDescRsp {
                    status: 0,
                    nwk,
                    descriptor: Some(d),
                },
                None => Body::SimpleDescRsp {
                    status: zigbee_zdo::status::INVALID_EP,
                    nwk,
                    descriptor: None,
                },
            },
            Body::BindReq(binding) => {
                let ok = binding.source == self.ieee
                    && self
                        .endpoints
                        .iter()
                        .any(|e| e.endpoint == binding.source_endpoint);
                if ok && !state.bindings.contains(&binding) {
                    state.bindings.push(binding);
                }
                Body::BindRsp {
                    status: if ok {
                        0
                    } else {
                        zigbee_zdo::status::NOT_SUPPORTED
                    },
                }
            }
            Body::UnbindReq(binding) => {
                let before = state.bindings.len();
                state.bindings.retain(|b| *b != binding);
                Body::UnbindRsp {
                    status: if state.bindings.len() < before {
                        0
                    } else {
                        zigbee_zdo::status::NO_ENTRY
                    },
                }
            }
            Body::MgmtLeaveReq { ieee, .. } if ieee == self.ieee || ieee == Ieee::ZERO => {
                reaction.leave = true;
                Body::MgmtLeaveRsp { status: 0 }
            }
            Body::MgmtLqiReq { .. } => Body::MgmtLqiRsp {
                status: 0,
                total: 1,
                start_index: 0,
                neighbors: vec![zigbee_zdo::Neighbor {
                    extended_pan_id: zigbee_types::ExtPanId(0),
                    ieee: coordinator,
                    nwk: 0,
                    flags: 0x04,
                    permit_joining: 2,
                    depth: 0,
                    lqi: self.lqi,
                }],
            },
            Body::MgmtPermitJoiningReq { .. } => Body::MgmtPermitJoiningRsp { status: 0 },
            Body::IeeeAddrReq { .. } => Body::IeeeAddrRsp {
                status: 0,
                ieee: self.ieee,
                nwk,
                associated: None,
            },
            Body::NwkAddrReq { .. } => Body::NwkAddrRsp {
                status: 0,
                ieee: self.ieee,
                nwk,
                associated: None,
            },
            _ => return reaction,
        };
        let (cluster, payload) = Message {
            tsn: request.tsn,
            body,
        }
        .encode();
        reaction.replies.push(Outgoing {
            profile: 0,
            cluster,
            source_endpoint: 0,
            destination_endpoint: 0,
            payload,
        });
        reaction
    }

    fn endpoint_mut(&mut self, endpoint: u8) -> Option<&mut SimEndpoint> {
        self.endpoints.iter_mut().find(|e| e.endpoint == endpoint)
    }

    /// Handle a ZCL frame addressed to one of this device's endpoints.
    /// Changed attributes that are configured for reporting are reported.
    pub fn handle_zcl(
        &mut self,
        state: &mut DeviceState,
        endpoint: u8,
        cluster: u16,
        source_endpoint: u8,
        payload: &[u8],
    ) -> Reaction {
        let mut reaction = Reaction::default();
        let Ok(frame) = Frame::parse(payload) else {
            return reaction;
        };
        let h = frame.header;
        let reply_to = |frame: Frame| Outgoing {
            profile: 0x0104,
            cluster,
            source_endpoint: endpoint,
            destination_endpoint: source_endpoint,
            payload: frame.encode(),
        };
        let default_response = |status: u8| {
            let mut f = Frame::global(
                Direction::ServerToClient,
                h.seq,
                foundation::id::DEFAULT_RESPONSE,
                Global::DefaultResponse {
                    command: h.command,
                    status,
                }
                .encode(),
            );
            f.header.disable_default_response = true;
            f
        };
        let Some(ep) = self.endpoint_mut(endpoint) else {
            return reaction;
        };
        let Some(attributes) = ep.servers.get_mut(&cluster) else {
            if !h.disable_default_response {
                reaction
                    .replies
                    .push(reply_to(default_response(status::UNSUPPORTED_CLUSTER)));
            }
            return reaction;
        };
        let mut changed = Vec::new();
        let response = match h.frame_type {
            FrameType::Global => match Global::decode(h.command, &frame.payload) {
                Ok(Global::ReadAttributes(ids)) => Some(Global::ReadAttributesResponse(
                    ids.into_iter()
                        .map(|id| match attributes.get(&id) {
                            Some(v) => ReadRecord {
                                attribute: id,
                                status: status::SUCCESS,
                                value: Some(v.clone()),
                            },
                            None => ReadRecord {
                                attribute: id,
                                status: status::UNSUPPORTED_ATTRIBUTE,
                                value: None,
                            },
                        })
                        .collect(),
                )),
                Ok(Global::WriteAttributes(values)) => {
                    let mut failures = Vec::new();
                    for (id, value) in values {
                        match attributes.get_mut(&id) {
                            Some(slot) => *slot = value,
                            None => failures.push((status::UNSUPPORTED_ATTRIBUTE, Some(id))),
                        }
                    }
                    Some(Global::WriteAttributesResponse(if failures.is_empty() {
                        vec![(status::SUCCESS, None)]
                    } else {
                        failures
                    }))
                }
                Ok(Global::ConfigureReporting(configs)) => {
                    let mut failures = Vec::new();
                    for c in configs {
                        if attributes.contains_key(&c.attribute) {
                            state.reporting.insert((endpoint, cluster, c.attribute), c);
                        } else {
                            failures.push(ReportingStatus {
                                status: status::UNSUPPORTED_ATTRIBUTE,
                                attribute: Some(c.attribute),
                            });
                        }
                    }
                    Some(Global::ConfigureReportingResponse(if failures.is_empty() {
                        vec![ReportingStatus {
                            status: status::SUCCESS,
                            attribute: None,
                        }]
                    } else {
                        failures
                    }))
                }
                Ok(_) => None,
                Err(_) => {
                    reaction
                        .replies
                        .push(reply_to(default_response(status::MALFORMED_COMMAND)));
                    return reaction;
                }
            },
            FrameType::Cluster => {
                let handled = match (cluster, h.command) {
                    (cluster_id::ON_OFF, 0x00..=0x02) => {
                        let current = attributes
                            .get(&attr::on_off::ON_OFF)
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        let next = match h.command {
                            0x00 => false,
                            0x01 => true,
                            _ => !current,
                        };
                        attributes.insert(attr::on_off::ON_OFF, Value::bool(next));
                        changed.push(attr::on_off::ON_OFF);
                        true
                    }
                    (cluster_id::IDENTIFY, 0x00) => true,
                    (cluster_id::IAS_ZONE, 0x00) => {
                        state.ias_enrolled = true;
                        if let [_, zone_id, ..] = frame.payload.as_slice() {
                            attributes.insert(attr::ias_zone::ZONE_ID, Value::u8(*zone_id));
                            attributes.insert(attr::ias_zone::ZONE_STATE, Value::enum8(1));
                        }
                        true
                    }
                    _ => false,
                };
                if !h.disable_default_response {
                    reaction.replies.push(reply_to(default_response(if handled {
                        status::SUCCESS
                    } else {
                        status::UNSUP_COMMAND
                    })));
                }
                None
            }
        };
        if let Some(g) = response {
            let mut f = Frame::global(Direction::ServerToClient, h.seq, g.command_id(), g.encode());
            f.header.disable_default_response = true;
            reaction.replies.push(reply_to(f));
        }
        for attribute in changed {
            reaction
                .replies
                .extend(self.report(state, endpoint, cluster, attribute));
        }
        reaction
    }

    /// Set an attribute; returns the report frames its reporting
    /// configuration and bindings call for.
    pub fn set_attribute(
        &mut self,
        state: &mut DeviceState,
        endpoint: u8,
        cluster: u16,
        attribute: u16,
        value: Value,
    ) -> Vec<Outgoing> {
        if let Some(attributes) = self
            .endpoint_mut(endpoint)
            .and_then(|e| e.servers.get_mut(&cluster))
        {
            attributes.insert(attribute, value);
        }
        self.report(state, endpoint, cluster, attribute)
    }

    fn report(
        &self,
        state: &mut DeviceState,
        endpoint: u8,
        cluster: u16,
        attribute: u16,
    ) -> Vec<Outgoing> {
        if !state
            .reporting
            .contains_key(&(endpoint, cluster, attribute))
        {
            return Vec::new();
        }
        let Some(value) = self
            .endpoints
            .iter()
            .find(|e| e.endpoint == endpoint)
            .and_then(|e| e.servers.get(&cluster))
            .and_then(|a| a.get(&attribute))
            .cloned()
        else {
            return Vec::new();
        };
        state
            .bindings
            .iter()
            .filter(|b| b.source_endpoint == endpoint && b.cluster == cluster)
            .filter_map(|b| match b.target {
                zigbee_zdo::BindTarget::Endpoint { endpoint: dst, .. } => Some(dst),
                zigbee_zdo::BindTarget::Group(_) => None,
            })
            .map(|dst| {
                state.seq = state.seq.wrapping_add(1);
                let mut f = Frame::global(
                    Direction::ServerToClient,
                    state.seq,
                    foundation::id::REPORT_ATTRIBUTES,
                    Global::ReportAttributes(vec![(attribute, value.clone())]).encode(),
                );
                f.header.disable_default_response = true;
                Outgoing {
                    profile: 0x0104,
                    cluster,
                    source_endpoint: endpoint,
                    destination_endpoint: dst,
                    payload: f.encode(),
                }
            })
            .collect()
    }

    /// The device announcement it sends after joining.
    pub fn announce(&self, nwk: NodeId) -> Outgoing {
        let capability = self.node_descriptor().mac_capabilities;
        let (cluster, payload) = Message {
            tsn: 0x80,
            body: Body::DeviceAnnce {
                nwk,
                ieee: self.ieee,
                capability,
            },
        }
        .encode();
        Outgoing {
            profile: 0,
            cluster,
            source_endpoint: 0,
            destination_endpoint: 0,
            payload,
        }
    }
}
