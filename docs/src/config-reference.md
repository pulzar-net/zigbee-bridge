# Configuration reference

<!-- Generated from the configuration types by crates/zigbee-bridge/tests/config_reference.rs. Do not edit by hand. -->

Every key the configuration file accepts. Unknown keys are an error, so a typo stops the bridge at startup instead of being ignored. This page is generated from the configuration types; a [JSON Schema](config.schema.json) generated from the same types lets editors with TOML schema support (for example Taplo or Even Better TOML) complete and check the file.

## `[serial]`

The coordinator's port.

This table is required.

| Key | Type | Default | Description |
|---|---|---|---|
| `port` | string | required | A serial device (`/dev/ttyUSB0`) or `tcp://host:port`. |
| `baud` | integer (from 0) | `115200` | Baud rate. |
| `flow_control` | `"none"`, `"hardware"`, `"software"` | `"none"` | `none`, `hardware` (RTS/CTS) or `software`. `"none"`: None. `"hardware"`: RTS/CTS. `"software"`: XON/XOFF. |
| `adapter` | `"auto"`, `"ezsp"`, `"znp"`, `"deconz"` | `"auto"` | The coordinator family: `auto` (probe for it), `ezsp`, `znp` or `deconz`. `"auto"`: Probe the port for each family. `"ezsp"`: Silicon Labs EmberZNet. `"znp"`: Texas Instruments Z-Stack. `"deconz"`: dresden elektronik deCONZ (ConBee, RaspBee). |

## `[mqtt]`

The MQTT broker.

| Key | Type | Default | Description |
|---|---|---|---|
| `host` | string | `"localhost"` | Broker host. |
| `port` | integer (0 to 65535) | `1883` | Broker port. |
| `username` | string | unset | User name, if the broker wants one. |
| `password_file` | string | unset | A file holding the password (so it is not in the configuration). |
| `client_id` | string | `"zigbee-bridge"` | Client id. |
| `base_topic` | string | `"zigbee"` | Prefix of every topic the bridge uses. |
| `keepalive` | integer (from 0) | `30` | Keep-alive, seconds. |
| `retain_state` | boolean | `true` | Publish device state retained, so a subscriber sees it at once. |

## `[homeassistant]`

Home Assistant discovery.

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | boolean | `true` | Publish discovery messages. |
| `discovery_prefix` | string | `"homeassistant"` | Home Assistant's discovery prefix. |
| `status_topic` | string | `"homeassistant/status"` | Home Assistant's birth topic; discovery is re-sent when it says "online". |

## `[network]`

The network formed on first start. After that the network file in the data directory is the source of truth.

| Key | Type | Default | Description |
|---|---|---|---|
| `channel` | integer (0 to 255) | `15` | Channel, 11 to 26. |
| `tx_power` | integer (-128 to 127) | unset | Transmit power in dBm, if not the firmware's default. |
| `replace_other` | boolean | `false` | Leave a different network found on the coordinator and form this one. Off by default: a coordinator with another network is never wiped without being told to. |

## `[join]`

Trust center join policy.

| Key | Type | Default | Description |
|---|---|---|---|
| `install_code_only` | boolean | `false` | Only devices with an install code may join. |
| `allow_legacy` | boolean | `true` | Accept pre-Zigbee 3.0 devices (they keep the well-known link key). |

## `[data]`

Where state lives.

| Key | Type | Default | Description |
|---|---|---|---|
| `dir` | string | `"/var/lib/zigbee-bridge"` | The data directory: network keys, device store, backup. |

## `[ota]`

Device firmware upgrades.

| Key | Type | Default | Description |
|---|---|---|---|
| `dir` | string | unset | A directory of OTA image files; `<data.dir>/ota` when unset. Images are offered only when an `ota_update` request asks for one. |
