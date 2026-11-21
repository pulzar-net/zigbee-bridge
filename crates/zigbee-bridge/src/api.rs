//! The MQTT API: every payload the bridge publishes or reads, as a type.
//!
//! The bridge serializes these and nothing else, so they are the contract.
//! With the `unstable-asyncapi-docs` feature each one also derives a JSON
//! Schema and an AsyncAPI message, and `tests/asyncapi.rs` assembles them
//! into the AsyncAPI description the docs site renders as the MQTT
//! reference. That feature is docs tooling, not API: it may change in any
//! release, and the bridge's own builds never turn it on.
//!
//! Topics are under the base topic (`mqtt.base_topic`, default `zigbee`).
//! `<device>` is the name a person gave the device, or its IEEE address as
//! `0x00124b0001020304`. Keys never appear on MQTT.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

#[cfg(feature = "unstable-asyncapi-docs")]
use asyncapi_rust::ToAsyncApiMessage;
#[cfg(feature = "unstable-asyncapi-docs")]
use schemars::JsonSchema;

// ---------------------------------------------------------------------------
// Availability

/// The bridge's availability, retained on `bridge/state`. The broker
/// publishes `offline` as the bridge's last will when the connection drops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    derive(JsonSchema, ToAsyncApiMessage)
)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    asyncapi(summary = "The bridge is online or offline")
)]
pub struct BridgeState {
    /// `online` while the bridge runs, `offline` once it stops or its
    /// connection drops.
    pub state: Availability,
}

/// Online or offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Running and connected to the coordinator.
    Online,
    /// Stopped, or the MQTT connection dropped.
    Offline,
}

impl BridgeState {
    /// The availability as the payload the bridge publishes.
    pub fn payload(state: Availability) -> Vec<u8> {
        serde_json::to_vec(&BridgeState { state }).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Bridge info and devices

/// The bridge, its coordinator and its network, retained on `bridge/info`.
/// Republished when any of it changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    derive(JsonSchema, ToAsyncApiMessage)
)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    asyncapi(summary = "The bridge's version, coordinator, network and join state")
)]
pub struct BridgeInfo {
    /// The bridge's version.
    pub version: String,
    /// The coordinator.
    pub coordinator: CoordinatorInfo,
    /// The Zigbee network.
    pub network: NetworkInfo,
    /// Devices may join now.
    pub permit_join: bool,
    /// Seconds joining stays open; 0 when closed.
    pub permit_join_timeout: u8,
}

/// The coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct CoordinatorInfo {
    /// Its IEEE address.
    pub ieee_address: String,
    /// The firmware family: `ezsp`, `znp` or `deconz`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The firmware version.
    pub firmware: String,
    /// The serial protocol version.
    pub protocol: u8,
}

/// The Zigbee network.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct NetworkInfo {
    /// The radio channel, 11 to 26.
    pub channel: u8,
    /// The PAN id.
    pub pan_id: u16,
    /// The extended PAN id, as hex.
    pub extended_pan_id: String,
    /// The network update id, raised by each channel change.
    pub update_id: u8,
}

/// Every device in the network, retained on `bridge/devices`. Republished
/// when one joins, leaves, is renamed or finishes its interview.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    derive(JsonSchema, ToAsyncApiMessage)
)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    asyncapi(summary = "Every device and its interview")
)]
#[serde(transparent)]
pub struct BridgeDevices(pub Vec<DeviceSummary>);

/// A device.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct DeviceSummary {
    /// Its IEEE address.
    pub ieee_address: String,
    /// Its current network address.
    pub network_address: u16,
    /// The name it is published under: the name a person gave it, or its
    /// IEEE address.
    pub friendly_name: String,
    /// From the Basic cluster.
    pub manufacturer: Option<String>,
    /// From the Basic cluster.
    pub model: Option<String>,
    /// From the Basic cluster: 1 mains, 3 battery, 4 DC; `null` until the
    /// interview reads it.
    pub power_source: Option<u8>,
    /// From the Basic cluster.
    pub software_build_id: Option<String>,
    /// From the Basic cluster.
    pub date_code: Option<String>,
    /// Its role, from the node descriptor; `null` until the interview reads
    /// it.
    #[serde(rename = "type")]
    pub kind: Option<LogicalType>,
    /// How far its interview got.
    pub interview: InterviewSummary,
    /// It has a Zigbee 3.0 unique trust center link key.
    pub unique_link_key: bool,
    /// Its endpoints, from the simple descriptors.
    pub endpoints: Vec<EndpointSummary>,
}

/// A device's role in the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LogicalType {
    /// The coordinator.
    Coordinator,
    /// A router: mains powered, relays for others.
    Router,
    /// An end device: often sleepy, talks through its parent.
    EndDevice,
    /// A reserved value.
    Unknown,
}

impl From<zigbee_zdo::LogicalType> for LogicalType {
    fn from(t: zigbee_zdo::LogicalType) -> Self {
        match t {
            zigbee_zdo::LogicalType::Coordinator => LogicalType::Coordinator,
            zigbee_zdo::LogicalType::Router => LogicalType::Router,
            zigbee_zdo::LogicalType::EndDevice => LogicalType::EndDevice,
            zigbee_zdo::LogicalType::Reserved(_) => LogicalType::Unknown,
        }
    }
}

/// How far an interview got.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct InterviewSummary {
    /// The stage it is at, or `done`.
    pub stage: InterviewStage,
    /// Every stage finished.
    pub done: bool,
    /// Why the interview stopped, if it did; it resumes when the device next
    /// talks.
    pub error: Option<String>,
}

/// An interview stage, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum InterviewStage {
    /// Reading the node descriptor.
    NodeDescriptor,
    /// Reading the endpoint list.
    ActiveEndpoints,
    /// Reading each endpoint's clusters.
    SimpleDescriptors,
    /// Reading the Basic cluster.
    Basic,
    /// Binding and configuring reporting.
    Configure,
    /// Finished.
    Done,
}

impl From<zigbee_host::InterviewStage> for InterviewStage {
    fn from(s: zigbee_host::InterviewStage) -> Self {
        use zigbee_host::InterviewStage as S;
        match s {
            S::NodeDescriptor => InterviewStage::NodeDescriptor,
            S::ActiveEndpoints => InterviewStage::ActiveEndpoints,
            S::SimpleDescriptors => InterviewStage::SimpleDescriptors,
            S::Basic => InterviewStage::Basic,
            S::Configure => InterviewStage::Configure,
            S::Done => InterviewStage::Done,
        }
    }
}

/// An endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct EndpointSummary {
    /// The endpoint number.
    pub endpoint: u8,
    /// The application profile (0x0104 Home Automation).
    pub profile_id: u16,
    /// The device type within the profile.
    pub device_id: u16,
    /// Server clusters, by name, or as hex when the bridge does not know
    /// the cluster.
    pub input_clusters: Vec<String>,
    /// Client clusters, by name or hex.
    pub output_clusters: Vec<String>,
}

// ---------------------------------------------------------------------------
// Events

/// Something that happened in the network, on `bridge/event`. Not
/// retained.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    derive(JsonSchema, ToAsyncApiMessage)
)]
#[serde(tag = "type", content = "data")]
pub enum BridgeEvent {
    /// Joining opened or closed.
    #[serde(rename = "permit_join")]
    #[cfg_attr(
        feature = "unstable-asyncapi-docs",
        asyncapi(summary = "Joining opened or closed")
    )]
    PermitJoin {
        /// Seconds it stays open; 0 when closed.
        time: u8,
        /// The router joining was opened through, when only one.
        device: Option<String>,
    },
    /// A device joined, or rejoined.
    #[serde(rename = "device_joined")]
    #[cfg_attr(
        feature = "unstable-asyncapi-docs",
        asyncapi(summary = "A device joined")
    )]
    DeviceJoined {
        /// Its IEEE address.
        ieee_address: String,
        /// Its network address.
        network_address: u16,
        /// A rejoin rather than a first join.
        rejoin: bool,
    },
    /// A device announced itself, after joining or a power cycle.
    #[serde(rename = "device_announce")]
    #[cfg_attr(
        feature = "unstable-asyncapi-docs",
        asyncapi(summary = "A device announced itself")
    )]
    DeviceAnnounce {
        /// Its IEEE address.
        ieee_address: String,
        /// Its network address.
        network_address: u16,
    },
    /// A device left the network.
    #[serde(rename = "device_leave")]
    #[cfg_attr(
        feature = "unstable-asyncapi-docs",
        asyncapi(summary = "A device left")
    )]
    DeviceLeave {
        /// Its IEEE address.
        ieee_address: String,
    },
    /// The bridge removed a device.
    #[serde(rename = "device_removed")]
    #[cfg_attr(
        feature = "unstable-asyncapi-docs",
        asyncapi(summary = "The bridge removed a device")
    )]
    DeviceRemoved {
        /// Its IEEE address.
        ieee_address: String,
        /// Why.
        reason: String,
    },
    /// An interview started, finished or failed.
    #[serde(rename = "device_interview")]
    #[cfg_attr(
        feature = "unstable-asyncapi-docs",
        asyncapi(summary = "An interview started, finished or failed")
    )]
    DeviceInterview(InterviewEvent),
    /// A device asked for a firmware update and the OTA directory has a
    /// newer image for it.
    #[serde(rename = "ota_available")]
    #[cfg_attr(
        feature = "unstable-asyncapi-docs",
        asyncapi(summary = "A firmware update is available")
    )]
    OtaAvailable {
        /// The device's name.
        device: String,
        /// Its IEEE address.
        ieee_address: String,
        /// The file version it runs.
        installed_version: u32,
        /// The newest version in the OTA directory.
        latest_version: u32,
    },
    /// A firmware download's progress.
    #[serde(rename = "ota_progress")]
    #[cfg_attr(
        feature = "unstable-asyncapi-docs",
        asyncapi(summary = "A firmware download's progress")
    )]
    OtaProgress {
        /// The device's name.
        device: String,
        /// Its IEEE address.
        ieee_address: String,
        /// Percent downloaded.
        progress: u8,
    },
    /// A firmware update ended.
    #[serde(rename = "ota_finished")]
    #[cfg_attr(
        feature = "unstable-asyncapi-docs",
        asyncapi(summary = "A firmware update ended")
    )]
    OtaFinished {
        /// The device's name.
        device: String,
        /// Its IEEE address.
        ieee_address: String,
        /// The version it ran.
        from_version: u32,
        /// The version it downloaded.
        to_version: u32,
        /// The device applied it.
        success: bool,
        /// The device's upgrade end status (ZCL status code).
        status: u8,
    },
    /// The network moved to another channel.
    #[serde(rename = "channel_changed")]
    #[cfg_attr(
        feature = "unstable-asyncapi-docs",
        asyncapi(summary = "The network changed channel")
    )]
    ChannelChanged {
        /// The new channel.
        channel: u8,
        /// The new network update id.
        update_id: u8,
    },
}

/// An interview's progress, by `status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
#[serde(tag = "status")]
pub enum InterviewEvent {
    /// The interview started or resumed.
    #[serde(rename = "started")]
    Started {
        /// The device's IEEE address.
        ieee_address: String,
    },
    /// The interview finished.
    #[serde(rename = "successful")]
    Successful {
        /// The device's IEEE address.
        ieee_address: String,
        /// From the Basic cluster.
        manufacturer: Option<String>,
        /// From the Basic cluster.
        model: Option<String>,
        /// The device, as on `bridge/devices`.
        definition: Box<DeviceSummary>,
    },
    /// The interview stopped; the bridge retries it later.
    #[serde(rename = "failed")]
    Failed {
        /// The device's IEEE address.
        ieee_address: String,
        /// The stage that failed.
        stage: InterviewStage,
        /// Why.
        error: String,
    },
}

// ---------------------------------------------------------------------------
// Devices

/// A device's state on `<device>`: its readings, merged as they arrive, plus
/// `linkquality`. Retained unless `mqtt.retain_state` is off; an empty
/// retained message clears it when the device leaves or is renamed.
///
/// Which keys appear depends on the device's clusters. A reading from a
/// second endpoint with the same cluster gets the endpoint number as a
/// suffix, as in `temperature_2`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(ToAsyncApiMessage))]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    asyncapi(summary = "A device's readings")
)]
#[serde(transparent)]
pub struct DeviceState(pub Map<String, Json>);

#[cfg(feature = "unstable-asyncapi-docs")]
impl JsonSchema for DeviceState {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DeviceState".into()
    }

    // The keys are the bridge's own table of standard attributes, so the
    // schema lists exactly what the bridge can publish.
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        use crate::exposes::{self, Convert, EXPOSES};
        use serde_json::json;
        let mut properties = Map::new();
        properties.insert(
            "state".into(),
            json!({"description": "On/off cluster: the switch or light is on.", "enum": ["ON", "OFF"]}),
        );
        properties.insert(
            "brightness".into(),
            json!({"description": "Level control cluster: the current level.", "type": "integer", "minimum": 0, "maximum": 254}),
        );
        for e in EXPOSES {
            let cluster = zigbee_zcl::cluster(e.cluster)
                .map(|c| c.name.replace('_', " "))
                .unwrap_or_else(|| format!("{:#06x}", e.cluster));
            let unit = e.unit.map(|u| format!(", in {u}")).unwrap_or_default();
            let schema = match e.convert {
                Convert::OnOff => json!({"enum": ["ON", "OFF"]}),
                Convert::Bit(_) => json!({"type": "boolean"}),
                Convert::Raw => json!({"type": "integer"}),
                Convert::Scale(..) | Convert::Lux => json!({"type": "number"}),
            };
            let mut schema = schema.as_object().cloned().unwrap_or_default();
            let description = format!("{}{unit}.", capitalize(&cluster));
            schema.insert("description".into(), description.into());
            properties
                .entry(e.field.to_string())
                .or_insert(Json::Object(schema));
        }
        for zone_type in [
            None,
            Some(0x0015),
            Some(0x000D),
            Some(0x002A),
            Some(0x0028),
            Some(0x002B),
            Some(0x002D),
        ] {
            for (field, _) in exposes::ias_fields(zone_type, 0) {
                properties.entry(field.to_string()).or_insert_with(|| {
                    json!({"description": format!("IAS zone: {}.", field.replace('_', " ")), "type": "boolean"})
                });
            }
        }
        properties.insert(
            "linkquality".into(),
            json!({"description": "The link quality of the device's last message, 0 to 255.", "type": "integer", "minimum": 0, "maximum": 255}),
        );
        schemars::json_schema!({
            "type": "object",
            "description": "The device's readings. Which keys appear depends on its clusters; a reading from a second endpoint with the same cluster has the endpoint number as a suffix, as in `temperature_2`.",
            "properties": properties,
            "additionalProperties": true,
        })
    }
}

#[cfg(feature = "unstable-asyncapi-docs")]
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// A command for a device, on `<device>/set`. Any combination of keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    derive(JsonSchema, ToAsyncApiMessage)
)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    asyncapi(summary = "Switch, dim or tune a device")
)]
pub struct SetRequest {
    /// `ON`, `OFF` or `TOGGLE`, in any case. `ON` together with
    /// `brightness` sets the level and turns the light on in one command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// The level, 0 to 254.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u64>,
    /// The color temperature in mireds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_temp: Option<u64>,
    /// How long a `brightness` or `color_temp` change takes, in seconds, to a
    /// tenth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<f64>,
}

/// An attribute report, raw, on `<device>/report`. Not retained.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    derive(JsonSchema, ToAsyncApiMessage)
)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    asyncapi(summary = "Every attribute report, raw, by cluster and attribute name")
)]
pub struct AttributeReport {
    /// The endpoint it came from.
    pub endpoint: u8,
    /// The cluster, by name, or as hex when the bridge does not know it.
    pub cluster: String,
    /// Each attribute's value, by name or hex id.
    pub attributes: Map<String, Json>,
}

/// A cluster command the device sent, such as a button press or an
/// alarm, on `<device>/action`. Not retained.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    derive(JsonSchema, ToAsyncApiMessage)
)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    asyncapi(summary = "A command the device sent: a button press, an alarm")
)]
pub struct DeviceAction {
    /// The endpoint it came from.
    pub endpoint: u8,
    /// The cluster, by name or hex.
    pub cluster: String,
    /// The command, by name, or as hex when the bridge does not know it.
    pub command: String,
    /// The command's fields, by name.
    pub fields: Map<String, Json>,
}

// ---------------------------------------------------------------------------
// Requests and responses

/// `ok` or `error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Done; `data` has the result.
    Ok,
    /// Not done; `error` says why.
    Error,
}

macro_rules! response {
    ($(#[$doc:meta])* $name:ident, $data:ty, $summary:literal) => {
        $(#[$doc])*
        ///
        /// A response is published once per request, to
        /// `bridge/response/<request>`, and is not retained.
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema, ToAsyncApiMessage))]
        #[cfg_attr(feature = "unstable-asyncapi-docs", asyncapi(summary = $summary))]
        pub struct $name {
            /// `ok` or `error`.
            pub status: Status,
            /// The result, when `status` is `ok`.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub data: Option<$data>,
            /// What went wrong, when `status` is `error`.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub error: Option<String>,
            /// The request's `transaction`, echoed when it had one.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub transaction: Option<Json>,
        }

        impl $name {
            /// The response to a request with `transaction`.
            pub fn new(result: Result<$data, String>, transaction: Option<Json>) -> Self {
                match result {
                    Ok(data) => $name { status: Status::Ok, data: Some(data), error: None, transaction },
                    Err(error) => $name { status: Status::Error, data: None, error: Some(error), transaction },
                }
            }
        }
    };
}

macro_rules! request {
    ($(#[$doc:meta])* $name:ident, $summary:literal { $($(#[$fdoc:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Default, Serialize, Deserialize)]
        #[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema, ToAsyncApiMessage))]
        #[cfg_attr(feature = "unstable-asyncapi-docs", asyncapi(summary = $summary))]
        pub struct $name {
            $($(#[$fdoc])* #[serde(default)] pub $field: $ty,)*
            /// Any value; the response echoes it, so a client can match the
            /// two.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub transaction: Option<Json>,
        }
    };
}

request!(
    /// Open or close joining, on `bridge/request/permit_join`.
    PermitJoinRequest, "Open or close joining" {
        /// Seconds to keep joining open; 0 closes it. Values over 254 are
        /// capped at 254.
        time: Option<u64>,
        /// `true` for 254 seconds, `false` to close; used when `time` is
        /// absent.
        value: Option<bool>,
        /// Open joining through this router only, by name or IEEE address.
        device: Option<String>,
    }
);

/// The result of `permit_join`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct PermitJoinData {
    /// Seconds joining stays open.
    pub time: u8,
}

response!(
    /// The response to `permit_join`.
    PermitJoinResponse, PermitJoinData, "Joining is open or closed"
);

request!(
    /// Remove a device, on `bridge/request/remove`.
    RemoveRequest, "Remove a device" {
        /// The device, by name or IEEE address (`id` also works).
        #[serde(alias = "id")]
        device: Option<String>,
        /// Forget the device even if it does not answer the leave request.
        force: bool,
    }
);

/// A device, by the name it was published under.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct DeviceData {
    /// The device's name.
    pub device: String,
}

response!(
    /// The response to `remove`.
    RemoveResponse, DeviceData, "The device is removed"
);

request!(
    /// Rename a device, on `bridge/request/rename`. Its topics move to the
    /// new name; the readings stay.
    RenameRequest, "Rename a device" {
        /// The device, by name or IEEE address (`device` also works).
        #[serde(alias = "device")]
        from: Option<String>,
        /// The new name (`name` also works). Not empty, no `+` or `#`, no
        /// leading or trailing `/`, not ending in `/set`, not `bridge`.
        #[serde(alias = "name")]
        to: Option<String>,
    }
);

/// The result of `rename`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct RenameData {
    /// The old name.
    pub from: String,
    /// The new name.
    pub to: String,
}

response!(
    /// The response to `rename`.
    RenameResponse, RenameData, "The device is renamed"
);

request!(
    /// Add an install code, on `bridge/request/install_code`, so the device
    /// with it can join.
    InstallCodeRequest, "Let a device join with its install code" {
        /// The device's IEEE address (`device` also works).
        #[serde(alias = "device")]
        ieee_address: Option<String>,
        /// The install code in hex, as printed with its CRC; spaces, dashes
        /// and colons are ignored.
        code: Option<String>,
    }
);

/// A device, by IEEE address.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct IeeeData {
    /// The device's IEEE address.
    pub ieee_address: String,
}

response!(
    /// The response to `install_code`.
    InstallCodeResponse, IeeeData, "The install code is added"
);

request!(
    /// Interview a device again, on `bridge/request/interview`.
    InterviewRequest, "Interview a device again" {
        /// The device, by name or IEEE address.
        device: Option<String>,
    }
);

response!(
    /// The response to `interview`; `bridge/event` follows the interview.
    InterviewResponse, DeviceData, "The interview is queued"
);

request!(
    /// Write a backup of the network to the data directory, on
    /// `bridge/request/backup`. The backup holds the network key, so it is
    /// never published.
    BackupRequest, "Back up the network" {}
);

/// The result of `backup`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct BackupData {
    /// Where the backup was written.
    pub path: String,
    /// How many devices it has.
    pub devices: usize,
    /// The network key's outgoing frame counter.
    pub frame_counter: u32,
}

response!(
    /// The response to `backup`.
    BackupResponse, BackupData, "The backup is written"
);

request!(
    /// The network's links as the bridge knows them, on
    /// `bridge/request/topology`.
    TopologyRequest, "The network's links" {}
);

/// The result of `topology`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct TopologyData {
    /// Every link.
    pub links: Vec<TopologyLink>,
}

/// A link from one device to another (towards the coordinator for links
/// learned from traffic).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct TopologyLink {
    /// The sending device's IEEE address, when known.
    pub from: Option<String>,
    /// The receiving device's IEEE address, when known.
    pub to: Option<String>,
    /// The sending device's network address.
    pub from_network_address: u16,
    /// The receiving device's network address.
    pub to_network_address: u16,
    /// Link quality, 0 to 255.
    pub lqi: Option<u8>,
    /// Signal strength in dBm.
    pub rssi: Option<i8>,
    /// Where the bridge learned the link.
    pub source: LinkSource,
    /// When it was last seen, in Unix seconds.
    pub seen: u64,
}

/// Where a link was learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LinkSource {
    /// The last hop of a message the coordinator received.
    LastHop,
    /// A hop in a route record.
    RouteRecord,
    /// A router's neighbor table.
    NeighborTable,
}

impl From<zigbee_host::topology::LinkSource> for LinkSource {
    fn from(s: zigbee_host::topology::LinkSource) -> Self {
        use zigbee_host::topology::LinkSource as S;
        match s {
            S::LastHop => LinkSource::LastHop,
            S::RouteRecord => LinkSource::RouteRecord,
            S::NeighborTable => LinkSource::NeighborTable,
        }
    }
}

response!(
    /// The response to `topology`.
    TopologyResponse, TopologyData, "The network's links"
);

request!(
    /// Ask a router for its neighbor table, on
    /// `bridge/request/scan_neighbors`.
    ScanNeighborsRequest, "Read a router's neighbor table" {
        /// The router, by name or IEEE address; the coordinator when absent.
        device: Option<String>,
    }
);

/// The result of `scan_neighbors`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct ScanNeighborsData {
    /// The router's neighbor table.
    pub neighbors: Vec<Neighbor>,
}

/// A neighbor table entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct Neighbor {
    /// The neighbor's extended PAN id, as hex.
    pub extended_pan_id: String,
    /// Its IEEE address, as hex without `0x`.
    pub ieee: String,
    /// Its network address.
    pub nwk: u16,
    /// Device type (bits 0-1), receiver on when idle (2-3), relationship
    /// (4-6).
    pub flags: u8,
    /// Permit joining (bits 0-1).
    pub permit_joining: u8,
    /// Tree depth.
    pub depth: u8,
    /// Link quality, 0 to 255.
    pub lqi: u8,
}

impl From<zigbee_zdo::Neighbor> for Neighbor {
    fn from(n: zigbee_zdo::Neighbor) -> Self {
        Neighbor {
            extended_pan_id: n.extended_pan_id.to_string(),
            ieee: n.ieee.to_string(),
            nwk: n.nwk,
            flags: n.flags,
            permit_joining: n.permit_joining,
            depth: n.depth,
            lqi: n.lqi,
        }
    }
}

response!(
    /// The response to `scan_neighbors`.
    ScanNeighborsResponse, ScanNeighborsData, "The router's neighbors"
);

request!(
    /// Ask whether the OTA directory has newer firmware for a device, on
    /// `bridge/request/ota_check`. A sleepy device answers when it next
    /// wakes, so the response can take a while.
    OtaCheckRequest, "Check a device for a firmware update" {
        /// The device, by name or IEEE address.
        device: Option<String>,
    }
);

/// The result of `ota_check` and `ota_update`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct OtaData {
    /// The device's name.
    pub device: String,
    /// The file version it runs.
    pub installed_version: u32,
    /// The newest newer version in the OTA directory.
    pub latest_version: Option<u32>,
    /// That image's size in bytes.
    pub size: Option<u32>,
    /// An update started; `bridge/event` reports its progress.
    pub updating: bool,
}

response!(
    /// The response to `ota_check`.
    OtaCheckResponse, OtaData, "Whether a firmware update is available"
);

request!(
    /// Update a device's firmware from the OTA directory, on
    /// `bridge/request/ota_update`. The response comes once the device
    /// starts the download; `bridge/event` reports the rest.
    OtaUpdateRequest, "Update a device's firmware" {
        /// The device, by name or IEEE address.
        device: Option<String>,
    }
);

response!(
    /// The response to `ota_update`.
    OtaUpdateResponse, OtaData, "The firmware update started"
);

request!(
    /// Move the network to another channel without pairing again, on
    /// `bridge/request/change_channel`.
    ChangeChannelRequest, "Move the network to another channel" {
        /// The new channel, 11 to 26.
        channel: Option<u64>,
    }
);

/// The result of `change_channel`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct ChannelData {
    /// The channel now.
    pub channel: u8,
    /// The network update id now.
    pub update_id: u8,
}

response!(
    /// The response to `change_channel`.
    ChangeChannelResponse, ChannelData, "The network changed channel"
);

request!(
    /// Measure the noise on every channel, on `bridge/request/energy_scan`.
    EnergyScanRequest, "Measure the noise on every channel" {}
);

/// The result of `energy_scan`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct EnergyScanData {
    /// Each channel's loudest signal.
    pub channels: Vec<ChannelEnergy>,
}

/// One channel's energy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct ChannelEnergy {
    /// The channel.
    pub channel: u8,
    /// The loudest signal on it, in dBm.
    pub max_rssi: i8,
}

response!(
    /// The response to `energy_scan`.
    EnergyScanResponse, EnergyScanData, "Each channel's noise"
);

request!(
    /// Publish `bridge/devices` again, on `bridge/request/devices`.
    DevicesRequest, "Publish the device list again" {}
);

/// An empty result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct Empty {}

response!(
    /// The response to `devices`.
    DevicesResponse, Empty, "The device list is published"
);

response!(
    /// The response to a request the bridge does not know; `error` names
    /// it.
    UnknownResponse, Empty, "The request is unknown"
);

// ---------------------------------------------------------------------------
// Home Assistant

/// Home Assistant's own availability, on its birth topic
/// (`homeassistant.status_topic`, default `homeassistant/status`). When it
/// says `online`, the bridge publishes discovery again. Plain text, not
/// JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    derive(JsonSchema, ToAsyncApiMessage)
)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    asyncapi(
        summary = "Home Assistant started or stopped",
        content_type = "text/plain"
    )
)]
#[serde(transparent)]
pub struct HomeAssistantStatus(pub HomeAssistantState);

/// Whether Home Assistant runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HomeAssistantState {
    /// Home Assistant started.
    Online,
    /// Home Assistant stopped.
    Offline,
}

impl HomeAssistantStatus {
    /// The status in a payload, if it is one.
    pub fn parse(payload: &[u8]) -> Option<HomeAssistantState> {
        match payload {
            b"online" => Some(HomeAssistantState::Online),
            b"offline" => Some(HomeAssistantState::Offline),
            _ => None,
        }
    }
}

/// A Home Assistant MQTT discovery config, retained on
/// `<discovery_prefix>/<component>/<ieee>/<field>/config`: one entity per
/// reading, light or switch. An empty retained message removes the entity.
/// Keys are Home Assistant's; see its MQTT integration documentation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    derive(JsonSchema, ToAsyncApiMessage)
)]
#[cfg_attr(
    feature = "unstable-asyncapi-docs",
    asyncapi(summary = "A Home Assistant entity for one of a device's readings")
)]
pub struct DiscoveryConfig {
    /// The entity's name.
    pub name: String,
    /// Unique across Home Assistant: `<ieee>_<field>_zigbee-bridge`.
    pub unique_id: String,
    /// The entity id Home Assistant derives: `<device>_<field>`.
    pub object_id: String,
    /// The device's state topic.
    pub state_topic: String,
    /// Picks the reading out of the state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_template: Option<String>,
    /// The bridge's availability.
    pub availability: Vec<DiscoveryAvailability>,
    /// The device the entity belongs to.
    pub device: DiscoveryDevice,
    /// The bridge, as the entity's origin.
    pub origin: DiscoveryOrigin,
    /// `json` for lights: state and commands are JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The device's `/set` topic, for lights and switches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_topic: Option<String>,
    /// Lights: supports brightness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<bool>,
    /// Lights: the highest level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness_scale: Option<u16>,
    /// Lights: `color_temp` when the light has color control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_color_modes: Option<Vec<String>>,
    /// The reading's unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_of_measurement: Option<String>,
    /// Home Assistant's device class for the reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_class: Option<String>,
    /// Home Assistant's state class for the reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_class: Option<String>,
    /// `diagnostic` for battery, link quality and tamper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_category: Option<String>,
    /// Binary sensors: the value that means on. Switches: the command that
    /// turns it on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_on: Option<DiscoveryPayload>,
    /// Binary sensors: the value that means off. Switches: the command that
    /// turns it off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_off: Option<DiscoveryPayload>,
    /// Switches: the state that means on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_on: Option<String>,
    /// Switches: the state that means off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_off: Option<String>,
    /// The entity's icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// A binary sensor's value or a switch's command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
#[serde(untagged)]
pub enum DiscoveryPayload {
    /// A binary sensor's value.
    Bool(bool),
    /// A switch's command, as JSON text.
    Text(String),
}

/// Where Home Assistant reads availability.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct DiscoveryAvailability {
    /// The bridge's `bridge/state`.
    pub topic: String,
    /// Picks `state` out of it.
    pub value_template: String,
}

/// The device an entity belongs to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct DiscoveryDevice {
    /// `zigbee-bridge_<ieee>`.
    pub identifiers: Vec<String>,
    /// The device's name.
    pub name: String,
    /// From the Basic cluster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    /// From the Basic cluster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// From the Basic cluster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sw_version: Option<String>,
}

/// The software that published the entity.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema))]
pub struct DiscoveryOrigin {
    /// `zigbee-bridge`.
    pub name: String,
    /// The bridge's version.
    pub sw_version: String,
}
