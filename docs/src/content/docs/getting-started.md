---
title: "Getting started"
description: "This page runs zigbee-bridge end to end without any Zigbee hardware: a simulated coordinator with three simulated devices takes the place of a USB stick."
---

This page runs zigbee-bridge end to end without any Zigbee hardware: a
simulated coordinator with three simulated devices takes the place of a USB
stick. Every step is the same with a real coordinator, except the port.

You need:

- Rust 1.89 or newer, to build the simulator (`rustup` from
  [rustup.rs](https://rustup.rs));
- an MQTT broker and its command-line clients, for example Mosquitto
  (`apt install mosquitto mosquitto-clients`, or `brew install mosquitto`);
- optionally, Home Assistant with its MQTT integration on the same broker.

## 1. Get the code and start the simulator

```sh
git clone https://github.com/pulzar-net/zigbee-bridge
cd zigbee-bridge
cargo run -p zigbee-sim --bin zigbee-ncp-sim -- \
    --listen 127.0.0.1:9999 --devices th,plug,door --report-every 10
```

The simulator plays a Silicon Labs EmberZNet coordinator on TCP port 9999.
Its three devices join as soon as the network is opened:

| `--devices` | IEEE address | What it is |
|---|---|---|
| `th` | `0x00124b0000000001` | temperature and humidity sensor (battery) |
| `plug` | `0x00124b0000000002` | smart plug (on/off, router) |
| `door` | `0x00124b0000000003` | door contact sensor (IAS zone) |

Addresses follow the order of `--devices`. With `--report-every 10`, the
sensor's temperature and humidity change every ten seconds.

## 2. Configure the bridge

In a second terminal, in the same directory, write `quickstart.toml`:

```toml
[serial]
port = "tcp://127.0.0.1:9999"

[mqtt]
host = "localhost"

[data]
dir = "./data"
```

`./data` keeps this trial out of `/var/lib`. The bridge creates it with mode
0700.

## 3. Run the bridge

```sh
cargo run -p zigbee-bridge -- --config quickstart.toml
```

(With a release binary, `zigbee-bridge --config quickstart.toml`.) The log
shows the network being formed:

```text
INFO zigbee_bridge: new network settings written channel=15 pan="fe5a"
INFO zigbee_host::ezsp_adapter: EZSP NCP connected protocol=13 firmware=7.4.4.0 build 380
INFO zigbee_host::host: network up outcome=Formed ieee=000d6ffffe000001 channel=15 pan="fe5a"
INFO zigbee_bridge::mqtt: MQTT connected
```

On this first start the bridge wrote `data/network.json` with a random PAN id
and network key. Every later start resumes that network.

## 4. Watch MQTT and pair

In a third terminal, watch everything under the base topic:

```sh
mosquitto_sub -h localhost -t 'zigbee/#' -v
```

Then open the network for joining.

```sh
mosquitto_pub -h localhost -t zigbee/bridge/request/permit_join -m '{"time": 120}'
```

Wait until the bridge has published `{"state":"online"}` on
`zigbee/bridge/state` before you do: it subscribes to requests as it comes
online, and a request sent earlier is lost. You will see, in order: the response on `zigbee/bridge/response/permit_join`,
a `device_joined` and a `device_interview` event per device on
`zigbee/bridge/event`, the updated device list on `zigbee/bridge/devices`,
and then readings:

```text
zigbee/0x00124b0000000001/report {"attributes":{"measured_value":1895},"cluster":"temperature_measurement","endpoint":1}
zigbee/0x00124b0000000001 {"humidity":45.6,"linkquality":180,"temperature":18.95}
```

If Home Assistant is connected to the same broker, the three devices are now
under **Settings > Devices & services > MQTT**.

## 5. Control and rename

Switch the plug on:

```sh
mosquitto_pub -h localhost -t zigbee/0x00124b0000000002/set -m '{"state":"ON"}'
```

Give the sensor a name; its topic becomes `zigbee/living_room`:

```sh
mosquitto_pub -h localhost -t zigbee/bridge/request/rename \
    -m '{"from": "0x00124b0000000001", "to": "living_room"}'
```

## Next steps

- [Install](/zigbee-bridge/installation.html) a release binary as a service.
- [Choose and flash a coordinator](/zigbee-bridge/hardware.html), then set `serial.port` to
  it. Use `zigbee-bridge probe PORT` first to check that it answers.
- Read the [MQTT API](/zigbee-bridge/mqtt.html) for every request and payload.
