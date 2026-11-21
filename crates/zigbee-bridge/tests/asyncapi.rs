//! The MQTT reference on the docs site is an AsyncAPI description generated
//! from the payload types in `zigbee_bridge::api`, the types the bridge
//! publishes, so it cannot drift from the code. This test fails when the
//! committed description differs from what the types say; regenerate it with
//!
//! ```sh
//! UPDATE_DOCS=1 cargo test -p zigbee-bridge --features unstable-asyncapi-docs --test asyncapi
//! ```
//!
//! Topics are written with the default base topic (`zigbee`) and discovery
//! prefix (`homeassistant`); the description says how to change them.

#![allow(clippy::duplicated_attributes)] // operations share channels by design

use std::path::PathBuf;

use asyncapi_rust::AsyncApi;
use zigbee_bridge::api::*;

const SPEC: &str = "docs/public/asyncapi.json";

/// The bridge's MQTT API.
#[derive(AsyncApi)]
#[asyncapi(
    title = "zigbee-bridge MQTT API",
    version = "0.0.0",
    description = "Everything zigbee-bridge publishes and accepts on MQTT.\n\nTopics here use the default base topic, `zigbee` (`mqtt.base_topic` in the configuration), and Home Assistant's default discovery prefix, `homeassistant` (`homeassistant.discovery_prefix`). `{device}` is the name a person gave a device, or its IEEE address as `0x00124b0001020304`.\n\nThe bridge publishes and subscribes at QoS 1. Payloads are JSON unless a message says otherwise. Requests go to `zigbee/bridge/request/<name>`; the answer comes on `zigbee/bridge/response/<name>`, echoing the request's `transaction` so a client can match the two. Network keys never appear on MQTT."
)]
#[asyncapi_server(
    name = "broker",
    host = "localhost:1883",
    protocol = "mqtt",
    description = "Your MQTT broker: `mqtt.host` and `mqtt.port` in the configuration. The bridge connects with a clean session, its `mqtt.client_id`, and `zigbee/bridge/state` set to offline as its last will.",
    mqtt(
        client_id = "zigbee-bridge",
        clean_session = true,
        keep_alive = 30,
        last_will(
            topic = "zigbee/bridge/state",
            qos = 1,
            message = "{\"state\":\"offline\"}",
            retain = true
        )
    )
)]
// Bridge state, info, devices and events.
#[asyncapi_channel(
    name = "bridgeState",
    address = "zigbee/bridge/state",
    description = "The bridge's availability, retained. `offline` is also the bridge's last will."
)]
#[asyncapi_channel(
    name = "bridgeInfo",
    address = "zigbee/bridge/info",
    description = "The bridge, its coordinator and its network, retained."
)]
#[asyncapi_channel(
    name = "bridgeDevices",
    address = "zigbee/bridge/devices",
    description = "Every device and its interview, retained."
)]
#[asyncapi_channel(
    name = "bridgeEvent",
    address = "zigbee/bridge/event",
    description = "Joins, announcements, departures, removals, interviews, firmware updates and channel changes, as they happen. `type` names the event and `data` holds it."
)]
// Devices.
#[asyncapi_channel(
    name = "deviceState",
    address = "zigbee/{device}",
    description = "A device's readings, merged as they arrive. Retained unless `mqtt.retain_state` is off. An empty retained message clears it when the device leaves or is renamed.",
    parameter(
        name = "device",
        description = "The device's name, or its IEEE address as `0x00124b0001020304`.",
        examples = ["kitchen_light", "0x00124b0001020304"]
    )
)]
#[asyncapi_channel(
    name = "deviceSet",
    address = "zigbee/{device}/set",
    description = "Commands for a device: switch it, dim it, set its color temperature.",
    parameter(
        name = "device",
        description = "The device's name, or its IEEE address as `0x00124b0001020304`.",
        examples = ["kitchen_light", "0x00124b0001020304"]
    )
)]
#[asyncapi_channel(
    name = "deviceReport",
    address = "zigbee/{device}/report",
    description = "Every attribute report the device sends, raw, by cluster and attribute name.",
    parameter(
        name = "device",
        description = "The device's name, or its IEEE address as `0x00124b0001020304`.",
        examples = ["kitchen_light", "0x00124b0001020304"]
    )
)]
#[asyncapi_channel(
    name = "deviceAction",
    address = "zigbee/{device}/action",
    description = "Cluster commands the device sends: button presses, scene recalls, alarms.",
    parameter(
        name = "device",
        description = "The device's name, or its IEEE address as `0x00124b0001020304`.",
        examples = ["kitchen_light", "0x00124b0001020304"]
    )
)]
// Requests and their responses.
#[asyncapi_channel(
    name = "permitJoinRequest",
    address = "zigbee/bridge/request/permit_join",
    description = "Open or close joining."
)]
#[asyncapi_channel(
    name = "permitJoinResponse",
    address = "zigbee/bridge/response/permit_join",
    description = "The answer to `permit_join`."
)]
#[asyncapi_channel(
    name = "removeRequest",
    address = "zigbee/bridge/request/remove",
    description = "Remove a device from the network."
)]
#[asyncapi_channel(
    name = "removeResponse",
    address = "zigbee/bridge/response/remove",
    description = "The answer to `remove`."
)]
#[asyncapi_channel(
    name = "renameRequest",
    address = "zigbee/bridge/request/rename",
    description = "Rename a device. Its topics move to the new name; its readings stay."
)]
#[asyncapi_channel(
    name = "renameResponse",
    address = "zigbee/bridge/response/rename",
    description = "The answer to `rename`."
)]
#[asyncapi_channel(
    name = "installCodeRequest",
    address = "zigbee/bridge/request/install_code",
    description = "Let a device join with its install code."
)]
#[asyncapi_channel(
    name = "installCodeResponse",
    address = "zigbee/bridge/response/install_code",
    description = "The answer to `install_code`."
)]
#[asyncapi_channel(
    name = "interviewRequest",
    address = "zigbee/bridge/request/interview",
    description = "Interview a device again."
)]
#[asyncapi_channel(
    name = "interviewResponse",
    address = "zigbee/bridge/response/interview",
    description = "The answer to `interview`."
)]
#[asyncapi_channel(
    name = "backupRequest",
    address = "zigbee/bridge/request/backup",
    description = "Write a backup of the network to the data directory. The backup holds the network key, so it is never published."
)]
#[asyncapi_channel(
    name = "backupResponse",
    address = "zigbee/bridge/response/backup",
    description = "The answer to `backup`."
)]
#[asyncapi_channel(
    name = "topologyRequest",
    address = "zigbee/bridge/request/topology",
    description = "The network's links as the bridge knows them."
)]
#[asyncapi_channel(
    name = "topologyResponse",
    address = "zigbee/bridge/response/topology",
    description = "The answer to `topology`."
)]
#[asyncapi_channel(
    name = "scanNeighborsRequest",
    address = "zigbee/bridge/request/scan_neighbors",
    description = "Read a router's neighbor table."
)]
#[asyncapi_channel(
    name = "scanNeighborsResponse",
    address = "zigbee/bridge/response/scan_neighbors",
    description = "The answer to `scan_neighbors`."
)]
#[asyncapi_channel(
    name = "otaCheckRequest",
    address = "zigbee/bridge/request/ota_check",
    description = "Check a device for a firmware update. A sleepy device answers when it next wakes."
)]
#[asyncapi_channel(
    name = "otaCheckResponse",
    address = "zigbee/bridge/response/ota_check",
    description = "The answer to `ota_check`."
)]
#[asyncapi_channel(
    name = "otaUpdateRequest",
    address = "zigbee/bridge/request/ota_update",
    description = "Update a device's firmware from the OTA directory."
)]
#[asyncapi_channel(
    name = "otaUpdateResponse",
    address = "zigbee/bridge/response/ota_update",
    description = "The answer to `ota_update`, once the device starts the download."
)]
#[asyncapi_channel(
    name = "changeChannelRequest",
    address = "zigbee/bridge/request/change_channel",
    description = "Move the network to another channel without pairing again."
)]
#[asyncapi_channel(
    name = "changeChannelResponse",
    address = "zigbee/bridge/response/change_channel",
    description = "The answer to `change_channel`."
)]
#[asyncapi_channel(
    name = "energyScanRequest",
    address = "zigbee/bridge/request/energy_scan",
    description = "Measure the noise on every channel."
)]
#[asyncapi_channel(
    name = "energyScanResponse",
    address = "zigbee/bridge/response/energy_scan",
    description = "The answer to `energy_scan`."
)]
#[asyncapi_channel(
    name = "devicesRequest",
    address = "zigbee/bridge/request/devices",
    description = "Publish `zigbee/bridge/devices` again."
)]
#[asyncapi_channel(
    name = "devicesResponse",
    address = "zigbee/bridge/response/devices",
    description = "The answer to `devices`."
)]
// Home Assistant.
#[asyncapi_channel(
    name = "homeAssistantDiscovery",
    address = "homeassistant/{component}/{node_id}/{object_id}/config",
    description = "Home Assistant MQTT discovery: one retained config per entity, sent when a device finishes its interview, when it is renamed, and when Home Assistant comes online. An empty retained message removes an entity. Only with `homeassistant.enabled`.",
    parameter(
        name = "component",
        description = "The entity type.",
        enum_values = ["sensor", "binary_sensor", "switch", "light"]
    ),
    parameter(
        name = "node_id",
        description = "The device's IEEE address, as hex without `0x`.",
        examples = ["00124b0001020304"]
    ),
    parameter(
        name = "object_id",
        description = "The reading: `temperature`, `battery`, `light`, `linkquality` and so on.",
        examples = ["temperature", "linkquality"]
    )
)]
#[asyncapi_channel(
    name = "homeAssistantStatus",
    address = "homeassistant/status",
    description = "Home Assistant's birth and last will (`homeassistant.status_topic`). When it says `online`, the bridge publishes discovery again."
)]
// What the bridge publishes.
#[asyncapi_operation(name = "publishBridgeState", action = "send", channel = "bridgeState", description = "Published `online` on connecting and `offline` on stopping.", messages = [BridgeState], mqtt(qos = 1, retain = true))]
#[asyncapi_operation(name = "publishBridgeInfo", action = "send", channel = "bridgeInfo", description = "Published on connecting and whenever the coordinator, the network or the join state changes.", messages = [BridgeInfo], mqtt(qos = 1, retain = true))]
#[asyncapi_operation(name = "publishBridgeDevices", action = "send", channel = "bridgeDevices", description = "Published on connecting and whenever a device joins, leaves, is renamed or finishes its interview.", messages = [BridgeDevices], mqtt(qos = 1, retain = true))]
#[asyncapi_operation(name = "publishBridgeEvent", action = "send", channel = "bridgeEvent", description = "Published as each event happens.", messages = [BridgeEvent], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "publishDeviceState", action = "send", channel = "deviceState", description = "Published after each reading.", messages = [DeviceState], mqtt(qos = 1, retain = true))]
#[asyncapi_operation(name = "publishDeviceReport", action = "send", channel = "deviceReport", description = "Published for each attribute report.", messages = [AttributeReport], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "publishDeviceAction", action = "send", channel = "deviceAction", description = "Published for each command the device sends.", messages = [DeviceAction], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "publishDiscovery", action = "send", channel = "homeAssistantDiscovery", description = "Published per entity.", messages = [DiscoveryConfig], mqtt(qos = 1, retain = true))]
// What the bridge receives.
#[asyncapi_operation(name = "receiveDeviceSet", action = "receive", channel = "deviceSet", description = "Any combination of keys in one message.", messages = [SetRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "receiveHomeAssistantStatus", action = "receive", channel = "homeAssistantStatus", description = "Read only with `homeassistant.enabled`.", messages = [HomeAssistantStatus], mqtt(qos = 1))]
#[asyncapi_operation(name = "receivePermitJoin", action = "receive", channel = "permitJoinRequest", messages = [PermitJoinRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendPermitJoin", action = "send", channel = "permitJoinResponse", messages = [PermitJoinResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveRemove", action = "receive", channel = "removeRequest", messages = [RemoveRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendRemove", action = "send", channel = "removeResponse", messages = [RemoveResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveRename", action = "receive", channel = "renameRequest", messages = [RenameRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendRename", action = "send", channel = "renameResponse", messages = [RenameResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveInstallCode", action = "receive", channel = "installCodeRequest", messages = [InstallCodeRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendInstallCode", action = "send", channel = "installCodeResponse", messages = [InstallCodeResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveInterview", action = "receive", channel = "interviewRequest", messages = [InterviewRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendInterview", action = "send", channel = "interviewResponse", messages = [InterviewResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveBackup", action = "receive", channel = "backupRequest", messages = [BackupRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendBackup", action = "send", channel = "backupResponse", messages = [BackupResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveTopology", action = "receive", channel = "topologyRequest", messages = [TopologyRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendTopology", action = "send", channel = "topologyResponse", messages = [TopologyResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveScanNeighbors", action = "receive", channel = "scanNeighborsRequest", messages = [ScanNeighborsRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendScanNeighbors", action = "send", channel = "scanNeighborsResponse", messages = [ScanNeighborsResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveOtaCheck", action = "receive", channel = "otaCheckRequest", messages = [OtaCheckRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendOtaCheck", action = "send", channel = "otaCheckResponse", messages = [OtaCheckResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveOtaUpdate", action = "receive", channel = "otaUpdateRequest", messages = [OtaUpdateRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendOtaUpdate", action = "send", channel = "otaUpdateResponse", messages = [OtaUpdateResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveChangeChannel", action = "receive", channel = "changeChannelRequest", messages = [ChangeChannelRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendChangeChannel", action = "send", channel = "changeChannelResponse", messages = [ChangeChannelResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveEnergyScan", action = "receive", channel = "energyScanRequest", messages = [EnergyScanRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendEnergyScan", action = "send", channel = "energyScanResponse", messages = [EnergyScanResponse], mqtt(qos = 1, retain = false))]
#[asyncapi_operation(name = "receiveDevices", action = "receive", channel = "devicesRequest", messages = [DevicesRequest], mqtt(qos = 1))]
#[asyncapi_operation(name = "sendDevices", action = "send", channel = "devicesResponse", messages = [DevicesResponse], mqtt(qos = 1, retain = false))]
struct MqttApi;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn render() -> String {
    let mut spec = MqttApi::asyncapi_spec();
    // The description is versioned with the bridge.
    spec.info.version = env!("CARGO_PKG_VERSION").into();
    serde_json::to_string_pretty(&spec).unwrap() + "\n"
}

#[test]
fn asyncapi_description_is_current() {
    let json = render();
    let path = repo_root().join(SPEC);
    if std::env::var_os("UPDATE_DOCS").is_some() {
        std::fs::write(&path, &json).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == json,
        "{SPEC} is stale: regenerate it with \
         UPDATE_DOCS=1 cargo test -p zigbee-bridge --features unstable-asyncapi-docs --test asyncapi"
    );
}
