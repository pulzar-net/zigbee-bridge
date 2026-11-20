# Home Assistant

zigbee-bridge works with Home Assistant through its
[MQTT integration](https://www.home-assistant.io/integrations/mqtt/) and
MQTT discovery. There is no custom integration to install.

## Setup

1. Run an MQTT broker. The
   [Mosquitto add-on](https://github.com/home-assistant/addons/tree/master/mosquitto)
   is the usual choice on Home Assistant OS; any broker works.
2. Add the MQTT integration in Home Assistant (**Settings > Devices &
   services > Add integration > MQTT**) and point it at the broker.
   Discovery is on by default.
3. Point zigbee-bridge at the same broker (`[mqtt] host`, `port`, and, if the
   broker needs them, `username` and `password_file`).
4. Start the bridge and [pair a device](getting-started.md#4-watch-mqtt-and-pair).

When a device finishes its interview, it appears under **Settings > Devices
& services > MQTT** with one entity per reading.

## What is created

Each device becomes one Home Assistant device, identified by its IEEE
address, with the manufacturer, model and software build the device reports.
Its entities:

| From | Entity | Notes |
|---|---|---|
| On/Off and Level Control on one endpoint | `light` | brightness 0 to 254; color temperature when the endpoint has Color Control; JSON schema |
| On/Off alone | `switch` | |
| Measurement clusters, power, battery | `sensor` | with unit, device class and `state_class: measurement` |
| Occupancy Sensing | `binary_sensor` | `motion` class |
| IAS Zone | `binary_sensor` | the zone's class (door, motion, moisture, smoke, carbon monoxide, vibration, safety), plus diagnostic `tamper` and `battery_low` |
| every device | `sensor` | link quality, as a diagnostic entity |

A battery reading is not created for mains-powered devices. The
[MQTT API](mqtt.md#device-state) lists every field and its cluster.

Entity ids are derived from the device's name, so renaming a device with the
`rename` request before it is added to dashboards gives readable ids such as
`sensor.living_room_temperature`. Unique ids are derived from the IEEE
address, so they survive renames.

Every entity uses `zigbee/bridge/state` for availability: when the bridge
stops or loses the broker, its devices show as unavailable.

## Topics

| Setting | Default | What |
|---|---|---|
| `homeassistant.discovery_prefix` | `homeassistant` | where discovery messages go, matching Home Assistant's discovery prefix |
| `homeassistant.status_topic` | `homeassistant/status` | Home Assistant's birth message; on `online` the bridge publishes discovery again |
| `homeassistant.enabled` | `true` | set `false` to publish no discovery at all |

Discovery topics have the form
`homeassistant/<component>/<ieee>/<field>/config`, for example
`homeassistant/sensor/00124b0000000001/temperature/config`. They are
retained, and republished when Home Assistant restarts.

## Removing a device

Remove it with the bridge's [`remove`](mqtt.md#remove) request, not only in
Home Assistant: the bridge clears its retained discovery messages and state,
so the entities disappear. Deleting the device only in Home Assistant leaves
it on the Zigbee network, and the bridge recreates it the next time Home
Assistant restarts.

## Using it with other software

Discovery is the only part that is specific to Home Assistant. Node-RED,
openHAB, scripts and anything else that speaks MQTT can use the
[topics](mqtt.md) directly; set `homeassistant.enabled = false` if nothing
uses discovery.
