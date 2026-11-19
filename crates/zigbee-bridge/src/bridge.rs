//! The bridge: host events to MQTT, MQTT requests to the host.
//!
//! Topics, under the base topic (default `zigbee`):
//!
//! | Topic | Retained | What |
//! |---|---|---|
//! | `bridge/state` | yes | `{"state":"online"}` / `"offline"` (last will) |
//! | `bridge/info` | yes | version, coordinator, network, join state |
//! | `bridge/devices` | yes | every device and its interview |
//! | `bridge/event` | no | joins, announces, leaves, removals, interviews |
//! | `bridge/request/<name>` | no | requests: `permit_join`, `remove`, `rename`, `install_code`, `interview`, `backup`, `topology`, `scan_neighbors`, `energy_scan` |
//! | `bridge/response/<name>` | no | `{"status":"ok","data":...}` or `{"status":"error","error":...}`, echoing `transaction` |
//! | `<device>` | configurable | the device's state: standard readings and `linkquality` |
//! | `<device>/set` | no | `{"state":"ON"}`, `{"brightness":128}`, `{"color_temp":300}` |
//! | `<device>/report` | no | every attribute report, raw, by cluster and attribute name |
//! | `<device>/action` | no | cluster commands the device sends (buttons, alarms) |
//!
//! `<device>` is the name a person gave it, or its IEEE address as
//! `0x00124b0001020304`. Keys never appear on MQTT.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::{json, Map, Value as Json};
use tokio::sync::mpsc;
use zigbee_host::{Device, EzspAdapter, Host, HostEvent, InterviewStage};
use zigbee_types::Ieee;
use zigbee_zcl::{cluster_id, Value};

use crate::config::Config;
use crate::exposes::{self, DiscoveryContext};
use crate::mqtt::{self, Incoming, Mqtt};

/// The bridge's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Bridge state between events.
pub struct Bridge {
    host: Host<EzspAdapter>,
    mqtt: Mqtt,
    config: Config,
    states: HashMap<Ieee, Map<String, Json>>,
    discovery: HashMap<Ieee, Vec<String>>,
    permit_join: u8,
}

fn ok(data: Json) -> Json {
    json!({"status": "ok", "data": data})
}

fn error(message: impl std::fmt::Display) -> Json {
    json!({"status": "error", "error": message.to_string()})
}

fn interview_name(stage: InterviewStage) -> &'static str {
    match stage {
        InterviewStage::NodeDescriptor => "node_descriptor",
        InterviewStage::ActiveEndpoints => "active_endpoints",
        InterviewStage::SimpleDescriptors => "simple_descriptors",
        InterviewStage::Basic => "basic",
        InterviewStage::Configure => "configure",
        InterviewStage::Done => "done",
    }
}

impl Bridge {
    /// A bridge over a running host and an MQTT session.
    pub fn new(host: Host<EzspAdapter>, mqtt: Mqtt, config: Config) -> Bridge {
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

    fn device_summary(device: &Device) -> Json {
        json!({
            "ieee_address": device.ieee.to_prefixed_hex(),
            "network_address": device.nwk,
            "friendly_name": Self::friendly(device),
            "manufacturer": device.manufacturer,
            "model": device.model,
            "power_source": device.power_source,
            "software_build_id": device.sw_build_id,
            "date_code": device.date_code,
            "type": device.node_descriptor.map(|d| match d.logical_type() {
                zigbee_zdo::LogicalType::Coordinator => "coordinator",
                zigbee_zdo::LogicalType::Router => "router",
                zigbee_zdo::LogicalType::EndDevice => "end_device",
                zigbee_zdo::LogicalType::Reserved(_) => "unknown",
            }),
            "interview": {"stage": interview_name(device.interview.stage), "done": device.interview.is_done(), "error": device.interview.error},
            "unique_link_key": device.unique_link_key,
            "endpoints": device.endpoints.iter().map(|e| json!({
                "endpoint": e.endpoint,
                "profile_id": e.profile_id,
                "device_id": e.device_id,
                "input_clusters": e.input_clusters.iter().map(|c| zigbee_zcl::cluster(*c).map(|d| d.name.to_string()).unwrap_or_else(|| format!("{c:#06x}"))).collect::<Vec<_>>(),
                "output_clusters": e.output_clusters.iter().map(|c| zigbee_zcl::cluster(*c).map(|d| d.name.to_string()).unwrap_or_else(|| format!("{c:#06x}"))).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    async fn publish_info(&self) {
        let n = self.host.network();
        let a = self.host.adapter_info();
        let info = json!({
            "version": VERSION,
            "coordinator": {"ieee_address": n.ieee.to_prefixed_hex(), "type": a.kind, "firmware": a.firmware, "protocol": a.protocol},
            "network": {"channel": n.channel, "pan_id": n.pan_id, "extended_pan_id": n.extended_pan_id.to_string()},
            "permit_join": self.permit_join > 0,
            "permit_join_timeout": self.permit_join,
        });
        self.mqtt
            .publish_json(format!("{}/bridge/info", self.base()), &info, true)
            .await;
    }

    async fn publish_devices(&self) {
        let devices: Vec<Json> = self
            .host
            .devices()
            .iter()
            .map(Self::device_summary)
            .collect();
        self.mqtt
            .publish_json(
                format!("{}/bridge/devices", self.base()),
                &Json::Array(devices),
                true,
            )
            .await;
    }

    async fn event(&self, kind: &str, data: Json) {
        self.mqtt
            .publish_json(
                format!("{}/bridge/event", self.base()),
                &json!({"type": kind, "data": data}),
                false,
            )
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
                    &Json::Object(state.clone()),
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
            .publish(mqtt::state_topic(&base), r#"{"state":"online"}"#, true)
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
                self.event(
                    "permit_join",
                    json!({"time": seconds, "device": via.map(|i| i.to_prefixed_hex())}),
                )
                .await;
            }
            HostEvent::DeviceJoined { ieee, nwk, rejoin } => {
                self.event("device_joined", json!({"ieee_address": ieee.to_prefixed_hex(), "network_address": nwk, "rejoin": rejoin})).await;
            }
            HostEvent::DeviceAnnounced { ieee, nwk } => {
                self.event(
                    "device_announce",
                    json!({"ieee_address": ieee.to_prefixed_hex(), "network_address": nwk}),
                )
                .await;
            }
            HostEvent::DeviceLeft { ieee } => {
                self.clear_device(ieee, &ieee.to_prefixed_hex()).await;
                self.event(
                    "device_leave",
                    json!({"ieee_address": ieee.to_prefixed_hex()}),
                )
                .await;
                self.publish_devices().await;
            }
            HostEvent::DeviceRemoved { ieee, reason } => {
                self.clear_device(ieee, &ieee.to_prefixed_hex()).await;
                self.event(
                    "device_removed",
                    json!({"ieee_address": ieee.to_prefixed_hex(), "reason": reason}),
                )
                .await;
                self.publish_devices().await;
            }
            HostEvent::InterviewStarted { ieee } => {
                self.event(
                    "device_interview",
                    json!({"ieee_address": ieee.to_prefixed_hex(), "status": "started"}),
                )
                .await;
            }
            HostEvent::InterviewStep { .. } => {}
            HostEvent::InterviewDone { device } => {
                self.event(
                    "device_interview",
                    json!({"ieee_address": device.ieee.to_prefixed_hex(), "status": "successful", "manufacturer": device.manufacturer, "model": device.model, "definition": Self::device_summary(&device)}),
                )
                .await;
                self.publish_devices().await;
                self.publish_discovery(&device).await;
            }
            HostEvent::InterviewFailed { ieee, stage, error } => {
                self.event("device_interview", json!({"ieee_address": ieee.to_prefixed_hex(), "status": "failed", "stage": interview_name(stage), "error": error})).await;
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
                let cluster_name = zigbee_zcl::cluster(cluster)
                    .map(|c| c.name.to_string())
                    .unwrap_or_else(|| format!("{cluster:#06x}"));
                let action = json!({"endpoint": endpoint, "cluster": cluster_name, "command": name.map(str::to_string).unwrap_or_else(|| format!("{command:#04x}")), "fields": fields});
                self.mqtt.publish_json(topic, &action, false).await;
            }
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
        let report = json!({"endpoint": endpoint, "cluster": def.map(|c| c.name.to_string()).unwrap_or_else(|| format!("{cluster:#06x}")), "attributes": raw});
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
            if payload == b"online" {
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
            let transaction = body.get("transaction").cloned();
            let mut response = self.request(name, &body).await;
            if let (Some(t), Some(o)) = (transaction, response.as_object_mut()) {
                o.insert("transaction".into(), t);
            }
            self.mqtt
                .publish_json(format!("{base}/bridge/response/{name}"), &response, false)
                .await;
        } else if let Some(device) = rest.strip_suffix("/set") {
            if let Err(e) = self.set(device, &body).await {
                tracing::warn!(device, error = %e, "set");
            }
        }
    }

    async fn request(&mut self, name: &str, body: &Json) -> Json {
        let device = |key: &str| body.get(key).and_then(Json::as_str).map(str::to_string);
        match name {
            "permit_join" => {
                let time = body
                    .get("time")
                    .and_then(Json::as_u64)
                    .or_else(|| {
                        body.get("value")
                            .and_then(Json::as_bool)
                            .map(|b| if b { 254 } else { 0 })
                    })
                    .unwrap_or(0);
                let via = match device("device") {
                    Some(name) => match self.resolve(&name) {
                        Some(d) => Some(d.ieee),
                        None => return error(format!("unknown device {name}")),
                    },
                    None => None,
                };
                match self.host.permit_join(time.min(254) as u8, via).await {
                    Ok(()) => ok(json!({"time": time.min(254)})),
                    Err(e) => error(e),
                }
            }
            "remove" => {
                let Some(d) = device("device")
                    .or(device("id"))
                    .and_then(|n| self.resolve(&n))
                else {
                    return error("unknown device");
                };
                let force = body.get("force").and_then(Json::as_bool).unwrap_or(false);
                let friendly = Self::friendly(&d);
                match self.host.remove(d.ieee, force).await {
                    Ok(()) => {
                        self.clear_device(d.ieee, &friendly).await;
                        ok(json!({"device": friendly}))
                    }
                    Err(e) => error(e),
                }
            }
            "rename" => {
                let Some(d) = device("from")
                    .or(device("device"))
                    .and_then(|n| self.resolve(&n))
                else {
                    return error("unknown device");
                };
                let Some(to) = device("to").or(device("name")) else {
                    return error("missing \"to\"");
                };
                if to.is_empty()
                    || to.contains(['+', '#'])
                    || to.starts_with('/')
                    || to.ends_with('/')
                    || to.ends_with("/set")
                    || to == "bridge"
                {
                    return error("not a usable name");
                }
                let old = Self::friendly(&d);
                // Clear the old retained topics, keep the readings.
                let state = self.states.remove(&d.ieee);
                self.clear_device(d.ieee, &old).await;
                if let Err(e) = self.host.set_name(d.ieee, Some(to.clone())) {
                    return error(e);
                }
                if let Some(state) = state {
                    self.states.insert(d.ieee, state);
                }
                if let Some(d) = self.host.device(d.ieee) {
                    self.publish_discovery(&d).await;
                    self.publish_state(&d).await;
                }
                self.publish_devices().await;
                ok(json!({"from": old, "to": to}))
            }
            "install_code" => {
                let Some(ieee) = device("ieee_address")
                    .or(device("device"))
                    .and_then(|t| t.parse::<Ieee>().ok())
                else {
                    return error("missing ieee_address");
                };
                let Some(code) = device("code") else {
                    return error("missing code");
                };
                let bytes = match zigbee_host::install_code::parse_hex(&code) {
                    Ok(b) => b,
                    Err(e) => return error(e),
                };
                match self.host.add_install_code(ieee, &bytes).await {
                    Ok(()) => ok(json!({"ieee_address": ieee.to_prefixed_hex()})),
                    Err(e) => error(e),
                }
            }
            "interview" => {
                let Some(d) = device("device").and_then(|n| self.resolve(&n)) else {
                    return error("unknown device");
                };
                match self.host.reinterview(d.ieee) {
                    Ok(()) => ok(json!({"device": Self::friendly(&d)})),
                    Err(e) => error(e),
                }
            }
            "backup" => match self.host.backup().await {
                // The backup holds the network key: it goes to the data
                // directory, never onto MQTT.
                Ok(b) => ok(
                    json!({"path": self.config.data.dir.join(crate::BACKUP_FILE).display().to_string(), "devices": b.devices.len(), "frame_counter": b.network_key.frame_counter}),
                ),
                Err(e) => error(e),
            },
            "topology" => {
                let links: Vec<Json> = self
                    .host
                    .topology()
                    .into_iter()
                    .map(|(from, to, l)| {
                        json!({
                            "from": from.map(|i| i.to_prefixed_hex()), "to": to.map(|i| i.to_prefixed_hex()),
                            "from_network_address": l.from, "to_network_address": l.to,
                            "lqi": l.lqi, "rssi": l.rssi, "source": l.source, "seen": l.seen_unix,
                        })
                    })
                    .collect();
                ok(json!({"links": links}))
            }
            "scan_neighbors" => {
                let target = match device("device") {
                    Some(n) => match self.resolve(&n) {
                        Some(d) => d.ieee,
                        None => return error(format!("unknown device {n}")),
                    },
                    None => self.host.network().ieee,
                };
                match self.host.scan_neighbors(target).await {
                    Ok(neighbors) => ok(json!({"neighbors": neighbors})),
                    Err(e) => error(e),
                }
            }
            "energy_scan" => match self.host.energy_scan().await {
                Ok(results) => ok(
                    json!({"channels": results.iter().map(|(c, r)| json!({"channel": c, "max_rssi": r})).collect::<Vec<_>>()}),
                ),
                Err(e) => error(e),
            },
            "devices" => {
                self.publish_devices().await;
                ok(json!({}))
            }
            other => error(format!("unknown request {other}")),
        }
    }

    async fn set(&mut self, name: &str, body: &Json) -> Result<(), String> {
        let device = self
            .resolve(name)
            .ok_or_else(|| format!("unknown device {name}"))?;
        let ep = |cluster: u16| device.endpoint_with(cluster).map(|e| e.endpoint);
        let transition = body
            .get("transition")
            .and_then(Json::as_f64)
            .map(|s| (s * 10.0).round().clamp(0.0, 65534.0) as u16)
            .unwrap_or(0);
        if let Some(level) = body.get("brightness").and_then(Json::as_u64) {
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
        if let Some(mireds) = body.get("color_temp").and_then(Json::as_u64) {
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
        if let Some(state) = body.get("state").and_then(Json::as_str) {
            let command = match state.to_ascii_uppercase().as_str() {
                "ON" => "on",
                "OFF" => "off",
                "TOGGLE" => "toggle",
                other => return Err(format!("state {other} is not ON, OFF or TOGGLE")),
            };
            if !(command == "on" && body.get("brightness").is_some()) {
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
        .publish(mqtt::state_topic(&base), r#"{"state":"offline"}"#, true)
        .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    bridge.mqtt.disconnect().await;
    if let Err(e) = bridge.host.shutdown().await {
        tracing::warn!(error = %e, "writing the device store");
    }
    result
}
