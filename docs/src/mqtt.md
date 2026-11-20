# MQTT API

The bridge publishes and listens under one base topic, `zigbee` unless
`mqtt.base_topic` says otherwise. Every payload is JSON. This page uses the
default base topic throughout.

## Topics at a glance

| Topic | Direction | Retained | What |
|---|---|---|---|
| `zigbee/bridge/state` | out | yes | whether the bridge is online |
| `zigbee/bridge/info` | out | yes | version, coordinator, network, join state |
| `zigbee/bridge/devices` | out | yes | every device and its interview |
| `zigbee/bridge/event` | out | no | joins, announces, interviews, leaves, removals |
| `zigbee/bridge/request/<name>` | in | | a request to the bridge |
| `zigbee/bridge/response/<name>` | out | no | the answer to a request |
| `zigbee/<device>` | out | per `mqtt.retain_state` (default yes) | the device's current state |
| `zigbee/<device>/set` | in | | a command to the device |
| `zigbee/<device>/report` | out | no | every attribute report, decoded by name |
| `zigbee/<device>/action` | out | no | commands the device sends |

`<device>` is the device's name if it has one (see [`rename`](#rename)), and
otherwise its IEEE address as `0x` and 16 lowercase hex digits, such as
`0x00124b0001020304`. Requests that take a device accept either form.

Network keys, link keys and install codes are never published.

## Bridge topics

### `bridge/state`

`{"state":"online"}` when the bridge is connected and running. The bridge
registers `{"state":"offline"}` as its MQTT last will, so the broker
publishes it if the bridge disappears, and publishes it itself on a clean
stop. Home Assistant uses this topic for availability.

### `bridge/info`

```json
{
  "version": "0.1.0",
  "coordinator": {
    "ieee_address": "0x000d6ffffe000001",
    "type": "ezsp",
    "firmware": "7.4.4.0",
    "protocol": 13
  },
  "network": { "channel": 15, "pan_id": 29950, "extended_pan_id": "28298b99d1d288ef", "update_id": 0 },
  "permit_join": false,
  "permit_join_timeout": 0
}
```

`coordinator.type` is the coordinator family; `protocol` is the version of
its serial protocol. `pan_id` is a number; `update_id` is the network update
id, which changes when the network changes channel. Published again whenever the join
window opens or closes.

### `bridge/devices`

An array with one entry per device:

```json
[
  {
    "ieee_address": "0x00124b0000000001",
    "network_address": 23456,
    "friendly_name": "living_room",
    "manufacturer": "zigbee-bridge-sim",
    "model": "SIM-TH1",
    "power_source": 3,
    "software_build_id": "1.0.0",
    "date_code": "20261119",
    "type": "end_device",
    "interview": { "stage": "done", "done": true, "error": null },
    "unique_link_key": true,
    "endpoints": [
      {
        "endpoint": 1,
        "profile_id": 260,
        "device_id": 770,
        "input_clusters": ["basic", "power_configuration", "identify", "temperature_measurement", "relative_humidity_measurement"],
        "output_clusters": ["ota_upgrade"]
      }
    ]
  }
]
```

| Field | Meaning |
|---|---|
| `type` | `router`, `end_device`, `coordinator`, `unknown`, or `null` before the interview reads it |
| `power_source` | the Basic cluster's power source: 1 mains, 3 battery, 4 DC; `null` if not read |
| `interview.stage` | the last step reached: `node_descriptor`, `active_endpoints`, `simple_descriptors`, `basic`, `configure`, `done` |
| `interview.error` | why the interview stopped, if it did; it resumes when the device next talks |
| `unique_link_key` | the device has a Zigbee 3.0 unique trust center link key |
| `input_clusters`, `output_clusters` | cluster names, or a hex id such as `"0xfc00"` for clusters the library does not name |

Published on connect, after every interview, leave and removal, and on the
`devices` request.

### `bridge/event`

```json
{ "type": "device_joined", "data": { "ieee_address": "0x00124b0000000001", "network_address": 23456, "rejoin": false } }
```

| `type` | `data` |
|---|---|
| `permit_join` | `time` (seconds open, 0 when closed), `device` (the router it was opened on, or `null`) |
| `device_joined` | `ieee_address`, `network_address`, `rejoin` |
| `device_announce` | `ieee_address`, `network_address` (sent after a join or a power cycle) |
| `device_interview` | `ieee_address`, `status`: `started`, `successful` (with `manufacturer`, `model` and `definition`, the device as in `bridge/devices`), or `failed` (with `stage` and `error`) |
| `device_leave` | `ieee_address` |
| `device_removed` | `ieee_address`, `reason` |
| `channel_changed` | `channel`, `update_id` (not in 0.1.0) |
| `ota_available` | `device`, `ieee_address`, `installed_version`, `latest_version`: a device asked for an update and a newer image is in the OTA directory (not in 0.1.0) |
| `ota_progress` | `device`, `ieee_address`, `progress` (percent) (not in 0.1.0) |
| `ota_finished` | `device`, `ieee_address`, `from_version`, `to_version`, `success`, `status` (not in 0.1.0) |

## Requests and responses

Publish a request to `zigbee/bridge/request/<name>`. The answer comes on
`zigbee/bridge/response/<name>`:

```json
{ "status": "ok", "data": { "time": 120 } }
{ "status": "error", "error": "unknown device kitchen" }
```

Put a `transaction` field in the request to have it copied into the
response, so you can match answers to questions:

```sh
mosquitto_pub -t zigbee/bridge/request/permit_join -m '{"time": 120, "transaction": "a1"}'
# zigbee/bridge/response/permit_join {"status":"ok","data":{"time":120},"transaction":"a1"}
```

An empty payload is the same as `{}`.

### `permit_join`

Open the network for joining, or close it.

| Field | Type | |
|---|---|---|
| `time` | integer, 0 to 254 | seconds to stay open; 0 closes. Values over 254 are capped at 254 |
| `value` | boolean | alternative to `time`: `true` is 254 seconds, `false` closes |
| `device` | string, optional | open only through this router |

```json
{ "time": 120 }
```

Response: `{"time": 120}`. A `permit_join` event follows, and another when
the window closes.

### `remove`

Ask a device to leave, and forget it.

| Field | Type | |
|---|---|---|
| `device` (or `id`) | string | name or IEEE address |
| `force` | boolean, default `false` | forget it even if it does not confirm (it is asleep or gone) |

Response: `{"device": "<name>"}`. Its retained state and Home Assistant
entities are cleared.

### `rename`

Give a device a name. Its state topic moves to `zigbee/<name>`, its old
retained topics are cleared, and its Home Assistant entities are published
again under the new name.

| Field | Type | |
|---|---|---|
| `from` (or `device`) | string | current name or IEEE address |
| `to` (or `name`) | string | the new name |

A name may contain `/` to group devices (`kitchen/ceiling`), but must not be
empty, contain `+` or `#`, start or end with `/`, end with `/set`, or be
`bridge`. Response: `{"from": "<old>", "to": "<new>"}`.

### `install_code`

Allow a device to join with its install code (printed on the device or its
box). With `join.install_code_only`, this is the only way a device can join.

| Field | Type | |
|---|---|---|
| `ieee_address` (or `device`) | string | the device's IEEE address |
| `code` | string | the install code in hex, as printed with its CRC; spaces, dashes and colons are ignored |

Response: `{"ieee_address": "..."}`. Install codes are held in memory only;
add them again after a restart if the device has not joined yet.

### `interview`

Interview a device again, for example after a firmware update changed its
clusters. Field: `device`. Response: `{"device": "<name>"}`; progress comes
as `device_interview` events.

### `backup`

Take a backup now. The backup holds the network key, so it is written to the
data directory and **never** published:

```json
{ "status": "ok", "data": { "path": "/var/lib/zigbee-bridge/backup.json", "devices": 3, "frame_counter": 60 } }
```

### `topology`

The links the coordinator knows about, from the last hop of messages it
received, route records and neighbor scans:

```json
{
  "links": [
    {
      "from": "0x00124b0000000001", "to": "0x00124b00aabbccdd",
      "from_network_address": 23456, "to_network_address": 0,
      "lqi": 160, "rssi": -62, "source": "last_hop", "seen": 1795136000
    }
  ]
}
```

`source` is `last_hop`, `route_record` or `neighbor_table`; `seen` is a Unix
time. `from` or `to` is `null` when only the network address is known. Links
older than a day are dropped.

### `scan_neighbors`

Ask a router (or, without `device`, the coordinator) for its neighbor table.
Field: `device`, optional. Response: `{"neighbors": [...]}`, one entry per
neighbor:

```json
{ "neighbors": [ { "ieee": "000d6ffffe000001", "nwk": 0, "lqi": 230, "depth": 0, "permit_joining": 2, "extended_pan_id": "0000000000000000", "flags": 4 } ] }
```

`ieee` here is 16 hex digits without `0x`; `flags` is the raw device type,
receiver and relationship byte of the ZDO neighbor table entry. The results also feed
`topology`.

### `change_channel`

Not in 0.1.0. Move the whole network to another channel, without pairing
again: the coordinator broadcasts a network update with the next update id,
then moves itself.

```json
{ "channel": 20 }
```

Response: `{"channel": 20, "update_id": 1}`, and a `channel_changed` event.
The new channel is saved to `network.json`. Sleepy end devices that miss the
broadcast find the network again when they next wake, which can take a
while; some need a power cycle.

### `ota_check` and `ota_update`

Not in 0.1.0. Device firmware updates from image files you provide. Put Zigbee
OTA image files in the OTA directory, `<data.dir>/ota` unless `ota.dir` says
otherwise. Any file name works: the bridge reads every file's OTA header,
including files where a vendor put its own header (up to 4 KB) in front.

`ota_check` asks a device for its firmware version and compares it with the
images in the directory; `ota_update` does the same, then sends the device
the newer image. Field: `device`. Nothing is ever offered to a device unless
`ota_update` asked for it.

```sh
mosquitto_pub -t zigbee/bridge/request/ota_update -m '{"device": "hallway_plug"}'
```

The response comes when the device answers, which for a sleepy device can be
its next wake-up:

```json
{ "status": "ok", "data": { "device": "hallway_plug", "installed_version": 16777216, "latest_version": 16842752, "size": 245760, "updating": true } }
```

Versions are the 32-bit file versions from the OTA header; `latest_version`
is `null` when the directory has nothing newer. Progress comes as
`ota_progress` events and the result as `ota_finished`. A transfer takes
minutes on a router and longer on a sleepy device.

> Needs real hardware: updates are tested against the simulator only.
> A failed update can leave a device unusable; try one device first.

### `energy_scan`

Measure the energy on every channel, to pick a quiet one:

```json
{ "channels": [ { "channel": 11, "max_rssi": -88 }, { "channel": 12, "max_rssi": -75 } ] }
```

### `devices`

Publish `bridge/devices` again. Response: `{}`.

## Device topics

### `<device>`: state

The device's current state, merged from every report and published after
each one:

```json
{ "humidity": 45.6, "linkquality": 180, "temperature": 18.95 }
```

Fields come from the standard clusters, so a device the bridge has never
seen still reports what it has:

| Field | Cluster | Unit |
|---|---|---|
| `temperature` | Temperature Measurement | °C |
| `humidity` | Relative Humidity Measurement | % |
| `pressure` | Pressure Measurement | hPa |
| `illuminance` | Illuminance Measurement | lx |
| `flow` | Flow Measurement | m³/h |
| `soil_moisture` | Soil Moisture Measurement | % |
| `co2` | Carbon Dioxide Measurement | ppm |
| `pm25` | PM2.5 Measurement | µg/m³ |
| `occupancy` | Occupancy Sensing | `true`/`false` |
| `state` | On/Off | `"ON"`/`"OFF"` |
| `brightness` | Level Control | 0 to 254 |
| `battery` | Power Configuration | % |
| `battery_voltage` | Power Configuration | mV |
| `power` | Electrical Measurement | W |
| `voltage` | Electrical Measurement | V |
| `current` | Electrical Measurement | A |
| `energy` | Metering | kWh |
| `local_temperature` | Thermostat | °C |
| `device_temperature` | Device Temperature Configuration | °C |
| `linkquality` | (every message) | 0 to 255 |

IAS zone devices (contact, motion, leak, smoke and similar sensors) add a
field named for their zone type, `contact`, `occupancy`, `water_leak`,
`smoke`, `carbon_monoxide`, `vibration`, or `alarm` for other types, plus
`tamper` and `battery_low`. `contact` is `true` when closed.

When a device has the same cluster on several endpoints (a double switch),
the first endpoint gives `state` and the others `state_2`, `state_3`, named
by endpoint number.

### `<device>/set`

| Field | Type | Effect |
|---|---|---|
| `state` | `"ON"`, `"OFF"`, `"TOGGLE"` (any case) | On/Off cluster |
| `brightness` | 0 to 254 | Level Control, move to level with on/off |
| `color_temp` | mireds | Color Control, move to color temperature |
| `transition` | seconds, decimals allowed | transition time for `brightness` and `color_temp` |

```sh
mosquitto_pub -t zigbee/kitchen/set -m '{"state": "ON", "brightness": 200, "transition": 1.5}'
```

Errors (an unknown device, a device without that cluster) are logged by the
bridge; there is no response topic for `set`.

### `<device>/report`

Every attribute report as received, for clusters and attributes the state
fields do not cover. Attributes are named from the cluster library, or
given as a hex id:

```json
{ "endpoint": 1, "cluster": "temperature_measurement", "attributes": { "measured_value": 1895 } }
```

Values are raw: `measured_value` 1895 is 18.95 °C.

### `<device>/action`

Commands the device sends, such as a remote's button presses:

```json
{ "endpoint": 1, "cluster": "on_off", "command": "toggle", "fields": {} }
```

`command` is the command's name, or a hex id for commands the library does
not name; `fields` holds its decoded fields.

## Home Assistant topics

With `homeassistant.enabled` (the default), the bridge also publishes
discovery messages under `homeassistant/` and listens on
`homeassistant/status`. See [Home Assistant](home-assistant.md).
