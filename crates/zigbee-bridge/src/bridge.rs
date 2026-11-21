//! The bridge: host events to MQTT, MQTT requests to the host.
//!
//! Every payload is a type in [`crate::api`]; the MQTT reference on the
//! docs site is generated from those types, topic by topic.

use std::collections::HashMap;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value as Json};
use tokio::sync::mpsc;
use zigbee_host::{AnyAdapter, Device, Host, HostEvent};
use zigbee_primitives::Ieee;
use zigbee_zcl::{cluster_id, Value};

use crate::api::{self, Availability, BridgeEvent, InterviewEvent};
use crate::config::Config;
use crate::exposes::{self, DiscoveryContext};
use crate::mqtt::{self, Incoming, Mqtt};

/// The bridge's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Bridge state between events.
pub struct Bridge {
    host: Host<AnyAdapter>,
    mqtt: Mqtt,
    config: Config,
    states: HashMap<Ieee, Map<String, Json>>,
    discovery: HashMap<Ieee, Vec<String>>,
    permit_join: u8,
}

fn to_json(value: &impl Serialize) -> Json {
    serde_json::to_value(value).unwrap_or(Json::Null)
}

/// A request's body as its type.
fn parse<T: DeserializeOwned>(name: &str, body: &Json) -> Result<T, String> {
    serde_json::from_value(body.clone()).map_err(|e| format!("invalid {name} request: {e}"))
}

fn cluster_name(cluster: u16) -> String {
    zigbee_zcl::cluster(cluster)
        .map(|c| c.name.to_string())
        .unwrap_or_else(|| format!("{cluster:#06x}"))
}

impl Bridge {
    /// A bridge over a running host and an MQTT session.
    pub fn new(host: Host<AnyAdapter>, mqtt: Mqtt, config: Config) -> Bridge {
        Bridge {
            host,
            mqtt,
            config,
            states: HashMap::new(),
            discovery: HashMap::new(),
            permit_join: 0,
        }
    }

    fn base(&self) -> &str {
        &self.config.mqtt.base_topic
    }

    fn friendly(device: &Device) -> String {
        device
            .name
            .clone()
            .unwrap_or_else(|| device.ieee.to_prefixed_hex())
    }

    fn name_of(&self, ieee: Ieee) -> String {
        self.host
            .device(ieee)
            .map(|d| Self::friendly(&d))
            .unwrap_or_else(|| ieee.to_prefixed_hex())
    }

    fn resolve(&self, name: &str) -> Option<Device> {
        if let Ok(ieee) = name.parse::<Ieee>() {
            if let Some(d) = self.host.device(ieee) {
                return Some(d);
            }
        }
        self.host
            .devices()
            .into_iter()
            .find(|d| d.name.as_deref() == Some(name))
    }

    fn device_summary(device: &Device) -> api::DeviceSummary {
        let clusters = |ids: &[u16]| ids.iter().map(|c| cluster_name(*c)).collect();
        api::DeviceSummary {
            ieee_address: device.ieee.to_prefixed_hex(),
            network_address: device.nwk,
            friendly_name: Self::friendly(device),
            manufacturer: device.manufacturer.clone(),
            model: device.model.clone(),
            power_source: device.power_source,
            software_build_id: device.sw_build_id.clone(),
            date_code: device.date_code.clone(),
            kind: device.node_descriptor.map(|d| d.logical_type().into()),
            interview: api::InterviewSummary {
                stage: device.interview.stage.into(),
                done: device.interview.is_done(),
                error: device.interview.error.clone(),
            },
            unique_link_key: device.unique_link_key,
            endpoints: device
                .endpoints
                .iter()
                .map(|e| api::EndpointSummary {
                    endpoint: e.endpoint,
                    profile_id: e.profile_id,
                    device_id: e.device_id,
                    input_clusters: clusters(&e.input_clusters),
                    output_clusters: clusters(&e.output_clusters),
                })
                .collect(),
        }
    }

    async fn publish_info(&self) {
        let n = self.host.network();
        let a = self.host.adapter_info();
        let info = api::BridgeInfo {
            version: VERSION.into(),
            coordinator: api::CoordinatorInfo {
                ieee_address: n.ieee.to_prefixed_hex(),
                kind: a.kind.into(),
                firmware: a.firmware,
                protocol: a.protocol,
            },
            network: api::NetworkInfo {
                channel: n.channel,
                pan_id: n.pan_id,
                extended_pan_id: n.extended_pan_id.to_string(),
                update_id: n.nwk_update_id,
            },
            permit_join: self.permit_join > 0,
            permit_join_timeout: self.permit_join,
        };
        self.mqtt
            .publish_json(format!("{}/bridge/info", self.base()), &info, true)
            .await;
    }

    async fn publish_devices(&self) {
        let devices = api::BridgeDevices(
            self.host
                .devices()
                .iter()
                .map(Self::device_summary)
                .collect(),
        );
        self.mqtt
            .publish_json(format!("{}/bridge/devices", self.base()), &devices, true)
            .await;
    }

    async fn event(&self, event: BridgeEvent) {
        self.mqtt
            .publish_json(format!("{}/bridge/event", self.base()), &event, false)
            .await;
    }

    async fn publish_discovery(&mut self, device: &Device) {
        if !self.config.homeassistant.enabled || !device.interview.is_done() {
            return;
        }
        let ctx = DiscoveryContext {
            prefix: &self.config.homeassistant.discovery_prefix,
            base: &self.config.mqtt.base_topic,
            version: VERSION,
        };
        let messages = exposes::discovery(&ctx, device, &Self::friendly(device));
        let topics: Vec<String> = messages.iter().map(|(t, _)| t.clone()).collect();
        for (topic, payload) in messages {
            self.mqtt.publish_json(topic, &payload, true).await;
        }
        // Clear entities this device no longer has.
        if let Some(old) = self.discovery.insert(device.ieee, topics.clone()) {
            for topic in old.into_iter().filter(|t| !topics.contains(t)) {
                self.mqtt.publish(topic, Vec::new(), true).await;
            }
        }
    }

    async fn clear_device(&mut self, ieee: Ieee, friendly: &str) {
        for topic in self.discovery.remove(&ieee).unwrap_or_default() {
            self.mqtt.publish(topic, Vec::new(), true).await;
        }
        self.mqtt
            .publish(format!("{}/{friendly}", self.base()), Vec::new(), true)
            .await;
        self.states.remove(&ieee);
    }

    async fn publish_state(&self, device: &Device) {
        if let Some(state) = self.states.get(&device.ieee) {
            let topic = format!("{}/{}", self.base(), Self::friendly(device));
            self.mqtt
                .publish_json(
                    topic,
                    &api::DeviceState(state.clone()),
                    self.config.mqtt.retain_state,
                )
                .await;
        }
    }

    /// Everything a fresh MQTT session needs: subscriptions, availability,
    /// retained info, devices and discovery.
    pub async fn on_connected(&mut self) {
        let base = self.base().to_string();
        self.mqtt
            .subscribe(&format!("{base}/bridge/request/+"))
            .await;
        self.mqtt.subscribe(&format!("{base}/+/set")).await;
        if self.config.homeassistant.enabled {
            let status = self.config.homeassistant.status_topic.clone();
            self.mqtt.subscribe(&status).await;
        }
        self.mqtt
            .publish(
                mqtt::state_topic(&base),
                api::BridgeState::payload(Availability::Online),
                true,
            )
            .await;
        self.publish_info().await;
        self.publish_devices().await;
        for device in self.host.devices() {
            self.publish_discovery(&device).await;
        }
    }

    /// Handle one host event.
    pub async fn on_host_event(&mut self, event: HostEvent) {
        match event {
            HostEvent::Started { .. } => {}
            HostEvent::PermitJoin { seconds, via } => {
                self.permit_join = seconds;
                self.publish_info().await;
                self.event(BridgeEvent::PermitJoin {
                    time: seconds,
                    device: via.map(|i| i.to_prefixed_hex()),
                })
                .await;
            }
            HostEvent::DeviceJoined { ieee, nwk, rejoin } => {
                self.event(BridgeEvent::DeviceJoined {
                    ieee_address: ieee.to_prefixed_hex(),
                    network_address: nwk,
                    rejoin,
                })
                .await;
            }
            HostEvent::DeviceAnnounced { ieee, nwk } => {
                self.event(BridgeEvent::DeviceAnnounce {
                    ieee_address: ieee.to_prefixed_hex(),
                    network_address: nwk,
                })
                .await;
            }
            HostEvent::OtaQuery {
                ieee,
                current_version,
                available,
            } => {
                if let Some(latest_version) = available {
                    self.event(BridgeEvent::OtaAvailable {
                        device: self.name_of(ieee),
                        ieee_address: ieee.to_prefixed_hex(),
                        installed_version: current_version,
                        latest_version,
                    })
                    .await;
                }
            }
            HostEvent::OtaProgress { ieee, percent } => {
                self.event(BridgeEvent::OtaProgress {
                    device: self.name_of(ieee),
                    ieee_address: ieee.to_prefixed_hex(),
                    progress: percent,
                })
                .await;
            }
            HostEvent::OtaFinished {
                ieee,
                from,
                to,
                success,
                status,
            } => {
                self.event(BridgeEvent::OtaFinished {
                    device: self.name_of(ieee),
                    ieee_address: ieee.to_prefixed_hex(),
                    from_version: from,
                    to_version: to,
                    success,
                    status,
                })
                .await;
            }
            HostEvent::ChannelChanged { network } => {
                // A re-formation after a coordinator reset uses the new
                // channel too.
                let dir = &self.config.data.dir;
                match crate::network::load_or_create(dir, network.channel, None) {
                    Ok((mut settings, _)) => {
                        settings.channel = network.channel;
                        if let Err(e) = crate::network::save(dir, &settings) {
                            tracing::warn!(error = %e, "saving the new channel");
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "reading the network file"),
                }
                self.publish_info().await;
                self.event(BridgeEvent::ChannelChanged {
                    channel: network.channel,
                    update_id: network.nwk_update_id,
                })
                .await;
            }
            HostEvent::DeviceLeft { ieee } => {
                self.clear_device(ieee, &ieee.to_prefixed_hex()).await;
                self.event(BridgeEvent::DeviceLeave {
                    ieee_address: ieee.to_prefixed_hex(),
                })
                .await;
                self.publish_devices().await;
            }
            HostEvent::DeviceRemoved { ieee, reason } => {
                self.clear_device(ieee, &ieee.to_prefixed_hex()).await;
                self.event(BridgeEvent::DeviceRemoved {
                    ieee_address: ieee.to_prefixed_hex(),
                    reason,
                })
                .await;
                self.publish_devices().await;
            }
            HostEvent::InterviewStarted { ieee } => {
                self.event(BridgeEvent::DeviceInterview(InterviewEvent::Started {
                    ieee_address: ieee.to_prefixed_hex(),
                }))
                .await;
            }
            HostEvent::InterviewStep { .. } => {}
            HostEvent::InterviewDone { device } => {
                self.event(BridgeEvent::DeviceInterview(InterviewEvent::Successful {
                    ieee_address: device.ieee.to_prefixed_hex(),
                    manufacturer: device.manufacturer.clone(),
                    model: device.model.clone(),
                    definition: Box::new(Self::device_summary(&device)),
                }))
                .await;
                self.publish_devices().await;
                self.publish_discovery(&device).await;
            }
            HostEvent::InterviewFailed { ieee, stage, error } => {
                self.event(BridgeEvent::DeviceInterview(InterviewEvent::Failed {
                    ieee_address: ieee.to_prefixed_hex(),
                    stage: stage.into(),
                    error,
                }))
                .await;
            }
            HostEvent::Attributes {
                ieee,
                endpoint,
                cluster,
                attributes,
                lqi,
            } => {
                self.on_attributes(ieee, endpoint, cluster, attributes, lqi)
                    .await
            }
            HostEvent::Command {
                ieee,
                endpoint,
                cluster,
                command,
                name,
                fields,
                ..
            } => {
                let Some(device) = self.host.device(ieee) else {
                    return;
                };
                if cluster == cluster_id::IAS_ZONE
                    && name == Some("zone_status_change_notification")
                {
                    if let Some(status) = fields
                        .iter()
                        .find(|(n, _)| *n == "zone_status")
                        .and_then(|(_, v)| v.as_u64())
                    {
                        self.apply_ias(&device, status as u16).await;
                    }
                }
                let fields: Map<String, Json> = fields
                    .into_iter()
                    .map(|(n, v)| {
                        (
                            n.to_string(),
                            serde_json::to_value(&v).unwrap_or(Json::Null),
                        )
                    })
                    .collect();
                let topic = format!("{}/{}/action", self.base(), Self::friendly(&device));
                let action = api::DeviceAction {
                    endpoint,
                    cluster: cluster_name(cluster),
                    command: name
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("{command:#04x}")),
                    fields,
                };
                self.mqtt.publish_json(topic, &action, false).await;
            }
            // The bridge publishes decoded readings; raw frames are for
            // embedders with their own device library.
            HostEvent::Frame { .. } => {}
            HostEvent::BackupWritten { path } => {
                tracing::info!(path = %path.display(), "backup written")
            }
            HostEvent::AdapterFailed { reason } => tracing::error!(%reason, "coordinator failed"),
        }
    }

    async fn apply_ias(&mut self, device: &Device, status: u16) {
        let state = self.states.entry(device.ieee).or_default();
        for (field, value) in exposes::ias_fields(device.ias_zone_type, status) {
            state.insert(field.to_string(), value);
        }
        self.publish_state(device).await;
    }

    async fn on_attributes(
        &mut self,
        ieee: Ieee,
        endpoint: u8,
        cluster: u16,
        attributes: Vec<(u16, Value)>,
        lqi: u8,
    ) {
        let Some(device) = self.host.device(ieee) else {
            return;
        };
        let def = zigbee_zcl::cluster(cluster);
        let raw: Map<String, Json> = attributes
            .iter()
            .map(|(id, v)| {
                let name = def
                    .and_then(|c| c.attribute(*id))
                    .map(|a| a.name.to_string())
                    .unwrap_or_else(|| format!("{id:#06x}"));
                (name, serde_json::to_value(v).unwrap_or(Json::Null))
            })
            .collect();
        let report = api::AttributeReport {
            endpoint,
            cluster: cluster_name(cluster),
            attributes: raw,
        };
        self.mqtt
            .publish_json(
                format!("{}/{}/report", self.base(), Self::friendly(&device)),
                &report,
                false,
            )
            .await;

        let state = self.states.entry(ieee).or_default();
        for (id, value) in &attributes {
            if cluster == cluster_id::IAS_ZONE && *id == zigbee_zcl::attr::ias_zone::ZONE_STATUS {
                if let Some(status) = value.as_u64() {
                    for (field, v) in exposes::ias_fields(device.ias_zone_type, status as u16) {
                        state.insert(field.to_string(), v);
                    }
                }
                continue;
            }
            if cluster == cluster_id::LEVEL_CONTROL
                && *id == zigbee_zcl::attr::level_control::CURRENT_LEVEL
            {
                if let Some(level) = value.as_u64() {
                    state.insert("brightness".into(), json!(level));
                }
                continue;
            }
            if let Some(e) = exposes::find(cluster, *id) {
                if let Some(v) = exposes::convert(e, value) {
                    state.insert(exposes::field_name(&device, endpoint, e), v);
                }
            }
        }
        state.insert("linkquality".into(), json!(lqi));
        self.publish_state(&device).await;
    }

    /// Handle one MQTT message.
    pub async fn on_message(&mut self, topic: &str, payload: &[u8]) {
        let base = self.base().to_string();
        if self.config.homeassistant.enabled && topic == self.config.homeassistant.status_topic {
            if api::HomeAssistantStatus::parse(payload) == Some(api::HomeAssistantState::Online) {
                for device in self.host.devices() {
                    self.publish_discovery(&device).await;
                }
            }
            return;
        }
        let Some(rest) = topic.strip_prefix(&format!("{base}/")) else {
            return;
        };
        let body: Json = if payload.is_empty() {
            json!({})
        } else {
            serde_json::from_slice(payload).unwrap_or(Json::Null)
        };
        if let Some(name) = rest.strip_prefix("bridge/request/") {
            // None: answered by a task of its own (a long OTA request).
            let Some(response) = self.request(name, &body).await else {
                return;
            };
            self.mqtt
                .publish_json(format!("{base}/bridge/response/{name}"), &response, false)
                .await;
        } else if let Some(device) = rest.strip_suffix("/set") {
            if let Err(e) = self.set(device, &body).await {
                tracing::warn!(device, error = %e, "set");
            }
        }
    }

    /// Answer a request; `None` when a task of its own answers it.
    async fn request(&mut self, name: &str, body: &Json) -> Option<Json> {
        let transaction = body.get("transaction").cloned();
        macro_rules! answer {
            ($response:ty, $handler:ident) => {{
                let result = match parse(name, body) {
                    Ok(request) => self.$handler(request).await,
                    Err(e) => Err(e),
                };
                to_json(&<$response>::new(result, transaction))
            }};
        }
        Some(match name {
            "permit_join" => answer!(api::PermitJoinResponse, permit_join),
            "remove" => answer!(api::RemoveResponse, remove),
            "rename" => answer!(api::RenameResponse, rename),
            "install_code" => answer!(api::InstallCodeResponse, install_code),
            "interview" => answer!(api::InterviewResponse, interview),
            "backup" => answer!(api::BackupResponse, backup),
            "topology" => answer!(api::TopologyResponse, topology),
            "scan_neighbors" => answer!(api::ScanNeighborsResponse, scan_neighbors),
            "change_channel" => answer!(api::ChangeChannelResponse, change_channel),
            "energy_scan" => answer!(api::EnergyScanResponse, energy_scan),
            "devices" => answer!(api::DevicesResponse, devices),
            "ota_check" => match parse::<api::OtaCheckRequest>(name, body) {
                Ok(r) => return self.ota(r.device, false, transaction),
                Err(e) => to_json(&api::OtaCheckResponse::new(Err(e), transaction)),
            },
            "ota_update" => match parse::<api::OtaUpdateRequest>(name, body) {
                Ok(r) => return self.ota(r.device, true, transaction),
                Err(e) => to_json(&api::OtaUpdateResponse::new(Err(e), transaction)),
            },
            other => to_json(&api::UnknownResponse::new(
                Err(format!("unknown request {other}")),
                transaction,
            )),
        })
    }

    fn device_named(&self, name: Option<String>) -> Result<Device, String> {
        match name {
            Some(n) => self
                .resolve(&n)
                .ok_or_else(|| format!("unknown device {n}")),
            None => Err("unknown device".into()),
        }
    }

    async fn permit_join(
        &mut self,
        r: api::PermitJoinRequest,
    ) -> Result<api::PermitJoinData, String> {
        let time = r
            .time
            .or(r.value.map(|open| if open { 254 } else { 0 }))
            .unwrap_or(0)
            .min(254) as u8;
        let via = match r.device {
            Some(name) => Some(self.device_named(Some(name))?.ieee),
            None => None,
        };
        self.host
            .permit_join(time, via)
            .await
            .map_err(|e| e.to_string())?;
        Ok(api::PermitJoinData { time })
    }

    async fn remove(&mut self, r: api::RemoveRequest) -> Result<api::DeviceData, String> {
        let d = self.device_named(r.device)?;
        let friendly = Self::friendly(&d);
        self.host
            .remove(d.ieee, r.force)
            .await
            .map_err(|e| e.to_string())?;
        self.clear_device(d.ieee, &friendly).await;
        Ok(api::DeviceData { device: friendly })
    }

    async fn rename(&mut self, r: api::RenameRequest) -> Result<api::RenameData, String> {
        let d = self.device_named(r.from)?;
        let to = r.to.ok_or("missing \"to\"")?;
        if to.is_empty()
            || to.contains(['+', '#'])
            || to.starts_with('/')
            || to.ends_with('/')
            || to.ends_with("/set")
            || to == "bridge"
        {
            return Err("not a usable name".into());
        }
        let old = Self::friendly(&d);
        // Clear the old retained topics, keep the readings.
        let state = self.states.remove(&d.ieee);
        self.clear_device(d.ieee, &old).await;
        self.host
            .set_name(d.ieee, Some(to.clone()))
            .map_err(|e| e.to_string())?;
        if let Some(state) = state {
            self.states.insert(d.ieee, state);
        }
        if let Some(d) = self.host.device(d.ieee) {
            self.publish_discovery(&d).await;
            self.publish_state(&d).await;
        }
        self.publish_devices().await;
        Ok(api::RenameData { from: old, to })
    }

    async fn install_code(&mut self, r: api::InstallCodeRequest) -> Result<api::IeeeData, String> {
        let ieee = r
            .ieee_address
            .and_then(|t| t.parse::<Ieee>().ok())
            .ok_or("missing ieee_address")?;
        let code = r.code.ok_or("missing code")?;
        let bytes = zigbee_host::install_code::parse_hex(&code).map_err(|e| e.to_string())?;
        self.host
            .add_install_code(ieee, &bytes)
            .await
            .map_err(|e| e.to_string())?;
        Ok(api::IeeeData {
            ieee_address: ieee.to_prefixed_hex(),
        })
    }

    async fn interview(&mut self, r: api::InterviewRequest) -> Result<api::DeviceData, String> {
        let d = self.device_named(r.device)?;
        self.host.reinterview(d.ieee).map_err(|e| e.to_string())?;
        Ok(api::DeviceData {
            device: Self::friendly(&d),
        })
    }

    async fn backup(&mut self, _: api::BackupRequest) -> Result<api::BackupData, String> {
        // The backup holds the network key: it goes to the data directory,
        // never onto MQTT.
        let b = self.host.backup().await.map_err(|e| e.to_string())?;
        Ok(api::BackupData {
            path: self
                .config
                .data
                .dir
                .join(crate::BACKUP_FILE)
                .display()
                .to_string(),
            devices: b.devices.len(),
            frame_counter: b.network_key.frame_counter,
        })
    }

    async fn topology(&mut self, _: api::TopologyRequest) -> Result<api::TopologyData, String> {
        let links = self
            .host
            .topology()
            .into_iter()
            .map(|(from, to, l)| api::TopologyLink {
                from: from.map(|i| i.to_prefixed_hex()),
                to: to.map(|i| i.to_prefixed_hex()),
                from_network_address: l.from,
                to_network_address: l.to,
                lqi: l.lqi,
                rssi: l.rssi,
                source: l.source.into(),
                seen: l.seen_unix,
            })
            .collect();
        Ok(api::TopologyData { links })
    }

    async fn scan_neighbors(
        &mut self,
        r: api::ScanNeighborsRequest,
    ) -> Result<api::ScanNeighborsData, String> {
        let target = match r.device {
            Some(n) => self.device_named(Some(n))?.ieee,
            None => self.host.network().ieee,
        };
        let neighbors = self
            .host
            .scan_neighbors(target)
            .await
            .map_err(|e| e.to_string())?;
        Ok(api::ScanNeighborsData {
            neighbors: neighbors.into_iter().map(Into::into).collect(),
        })
    }

    /// Check or update a device's firmware. A sleepy device answers when it
    /// next wakes: answer from a task so the bridge keeps working meanwhile.
    fn ota(&self, device: Option<String>, update: bool, transaction: Option<Json>) -> Option<Json> {
        let respond = move |result, transaction| {
            if update {
                to_json(&api::OtaUpdateResponse::new(result, transaction))
            } else {
                to_json(&api::OtaCheckResponse::new(result, transaction))
            }
        };
        let d = match self.device_named(device) {
            Ok(d) => d,
            Err(_) => return Some(respond(Err("unknown device".into()), transaction)),
        };
        let host = self.host.clone();
        let mqtt = self.mqtt.clone();
        let name = if update { "ota_update" } else { "ota_check" };
        let topic = format!("{}/bridge/response/{name}", self.base());
        let friendly = Self::friendly(&d);
        tokio::spawn(async move {
            let result = if update {
                host.ota_update(d.ieee).await
            } else {
                host.ota_check(d.ieee).await
            };
            let result = result
                .map(|o| api::OtaData {
                    device: friendly,
                    installed_version: o.current_version,
                    latest_version: o.available,
                    size: o.size,
                    updating: update,
                })
                .map_err(|e| e.to_string());
            mqtt.publish_json(topic, &respond(result, transaction), false)
                .await;
        });
        None
    }

    async fn change_channel(
        &mut self,
        r: api::ChangeChannelRequest,
    ) -> Result<api::ChannelData, String> {
        let channel = r
            .channel
            .ok_or("change_channel needs {\"channel\": 11..26}")?;
        let n = self
            .host
            .change_channel(channel.min(255) as u8)
            .await
            .map_err(|e| e.to_string())?;
        Ok(api::ChannelData {
            channel: n.channel,
            update_id: n.nwk_update_id,
        })
    }

    async fn energy_scan(
        &mut self,
        _: api::EnergyScanRequest,
    ) -> Result<api::EnergyScanData, String> {
        let results = self.host.energy_scan().await.map_err(|e| e.to_string())?;
        Ok(api::EnergyScanData {
            channels: results
                .into_iter()
                .map(|(channel, max_rssi)| api::ChannelEnergy { channel, max_rssi })
                .collect(),
        })
    }

    async fn devices(&mut self, _: api::DevicesRequest) -> Result<api::Empty, String> {
        self.publish_devices().await;
        Ok(api::Empty {})
    }

    async fn set(&mut self, name: &str, body: &Json) -> Result<(), String> {
        let device = self
            .resolve(name)
            .ok_or_else(|| format!("unknown device {name}"))?;
        let r: api::SetRequest =
            serde_json::from_value(body.clone()).map_err(|e| format!("invalid set: {e}"))?;
        let ep = |cluster: u16| device.endpoint_with(cluster).map(|e| e.endpoint);
        let transition = r
            .transition
            .map(|s| (s * 10.0).round().clamp(0.0, 65534.0) as u16)
            .unwrap_or(0);
        if let Some(level) = r.brightness {
            let endpoint = ep(cluster_id::LEVEL_CONTROL).ok_or("no level control")?;
            let level = level.min(254) as u8;
            self.host
                .command(
                    device.ieee,
                    endpoint,
                    cluster_id::LEVEL_CONTROL,
                    "move_to_level_with_on_off",
                    &[Value::u8(level), Value::u16(transition)],
                )
                .await
                .map_err(|e| e.to_string())?;
            self.states
                .entry(device.ieee)
                .or_default()
                .insert("brightness".into(), json!(level));
        }
        if let Some(mireds) = r.color_temp {
            let endpoint = ep(cluster_id::COLOR_CONTROL).ok_or("no color control")?;
            self.host
                .command(
                    device.ieee,
                    endpoint,
                    cluster_id::COLOR_CONTROL,
                    "move_to_color_temperature",
                    &[
                        Value::u16(mireds.min(0xFEFF) as u16),
                        Value::u16(transition),
                    ],
                )
                .await
                .map_err(|e| e.to_string())?;
        }
        if let Some(state) = &r.state {
            let command = match state.to_ascii_uppercase().as_str() {
                "ON" => "on",
                "OFF" => "off",
                "TOGGLE" => "toggle",
                other => return Err(format!("state {other} is not ON, OFF or TOGGLE")),
            };
            if !(command == "on" && r.brightness.is_some()) {
                let endpoint = ep(cluster_id::ON_OFF).ok_or("no on/off cluster")?;
                self.host
                    .command(device.ieee, endpoint, cluster_id::ON_OFF, command, &[])
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

/// Run the bridge until `stop` resolves or the coordinator fails.
pub async fn run_loop(
    mut bridge: Bridge,
    mut host_events: mpsc::UnboundedReceiver<HostEvent>,
    mut mqtt_in: mpsc::Receiver<Incoming>,
    stop: impl std::future::Future<Output = ()>,
) -> Result<(), String> {
    tokio::pin!(stop);
    let result = loop {
        tokio::select! {
            event = host_events.recv() => match event {
                Some(HostEvent::AdapterFailed { reason }) => break Err(format!("coordinator failed: {reason}")),
                Some(event) => bridge.on_host_event(event).await,
                None => break Err("host stopped".into()),
            },
            incoming = mqtt_in.recv() => match incoming {
                Some(Incoming::Connected) => bridge.on_connected().await,
                Some(Incoming::Message { topic, payload }) => bridge.on_message(&topic, &payload).await,
                None => break Err("MQTT session ended".into()),
            },
            _ = &mut stop => break Ok(()),
        }
    };
    let base = bridge.base().to_string();
    bridge
        .mqtt
        .publish(
            mqtt::state_topic(&base),
            api::BridgeState::payload(Availability::Offline),
            true,
        )
        .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    bridge.mqtt.disconnect().await;
    if let Err(e) = bridge.host.shutdown().await {
        tracing::warn!(error = %e, "writing the device store");
    }
    result
}
