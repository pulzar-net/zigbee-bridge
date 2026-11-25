# zigbee-bridge

[![CI](https://github.com/pulzar-net/zigbee-bridge/actions/workflows/ci.yml/badge.svg)](https://github.com/pulzar-net/zigbee-bridge/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pulzar-net/zigbee-bridge)](https://github.com/pulzar-net/zigbee-bridge/releases/latest)
[![crates.io](https://img.shields.io/crates/v/zigbee-host)](https://crates.io/crates/zigbee-host)
[![docs.rs](https://img.shields.io/docsrs/zigbee-host)](https://docs.rs/zigbee-host)
[![Docs](https://img.shields.io/badge/docs-oss.pulzr.net-blue)](https://oss.pulzr.net/zigbee-bridge/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue)](Cargo.toml)

A Zigbee coordinator stack and Zigbee to MQTT bridge, written in Rust. It
runs a Zigbee network through a USB or network coordinator, interviews the
devices that join, and publishes their readings to MQTT with Home Assistant
discovery. It is one static binary with no runtime: CI holds it under 4 MB,
and the stack under 16 MB of memory with 200 devices, so it fits on routers
and small gateways as well as on a Raspberry Pi or a server. The same stack is a set of library crates for
anyone building their own Zigbee software in Rust.

It exists because the established Zigbee stacks need a Node.js or Python
runtime, and there was no complete, embeddable Zigbee coordinator stack in
Rust.

**[Documentation](https://oss.pulzr.net/zigbee-bridge/)** |
**[Quick start](#quick-start)** |
**[Hardware](#supported-hardware)** |
**[MQTT API](https://oss.pulzr.net/zigbee-bridge/mqtt.html)** |
**[Library](#use-it-as-a-library)** |
**[Releases](https://github.com/pulzar-net/zigbee-bridge/releases)**

## Supported hardware

The coordinator is a radio module with network co-processor (NCP) firmware.
**Expected** means the hardware runs firmware the stack implements and is
covered by the simulator, but nobody has reported it working yet. If you try
one, please [tell us how it went](https://github.com/pulzar-net/zigbee-bridge/issues/new?template=device.yml).

| Coordinator | Chip | Firmware | State |
|---|---|---|---|
| Home Assistant Connect ZBT-1 (SkyConnect) | EFR32MG21 | EmberZNet 7.4 or newer, NCP | expected |
| Sonoff ZBDongle-E | EFR32MG21 | EmberZNet 6.10 as shipped, or 7.4 or newer, NCP | expected |
| SMLIGHT SLZB-06M, SLZB-07 | EFR32MG21 | EmberZNet 7.4 or newer, NCP (SLZB-06M over `tcp://`) | expected |
| Any EFR32MG21 or MG24 coordinator | EFR32MG2x | EmberZNet 7.4 to 8.x, NCP | expected |
| Sonoff ZBDongle-P, SMLIGHT SLZB-06 | CC2652P | Z-Stack 3.x.0 | expected |
| ConBee II, ConBee III, RaspBee II | deCONZ | deCONZ firmware 0x26580700 or newer | expected |
| Silicon Labs EM357 | EM35x | EmberZNet 6.7.10 (EZSP 8), NCP | working (Control4 CA-1, onboard UART) |
| EmberZNet 7.0 to 7.3, and 6.6 and older | EFR32, EM35x | EZSP 9 to 12, and 7 and older | unsupported: refused with "update the firmware" |
| CC2531, CC2530 | CC253x | Z-Stack Home 1.2 | unsupported: refused (`probe` still identifies it) |
| XBee, ZiGate, Telink, NXP | | | unsupported, not planned |

Devices: any Zigbee 3.0 or Zigbee HA 1.2 device that uses the standard
clusters reports its readings without device-specific code. The
[hardware page](https://oss.pulzr.net/zigbee-bridge/hardware.html)
lists what is exposed and how to flash recommended firmware.

## Quick start

From nothing to a sensor in Home Assistant, on 64-bit Linux (x86_64 or
aarch64) with a supported coordinator. You need an MQTT broker (for
example Mosquitto) that Home Assistant's MQTT integration already uses.

**1. Download the release and check its checksum.**

<!-- quickstart:download -->
```sh
VERSION=0.3.0
ARCH=$(uname -m)   # x86_64 or aarch64
BASE=https://github.com/pulzar-net/zigbee-bridge/releases/download/v$VERSION
curl -fsSLO "$BASE/zigbee-bridge-v$VERSION-$ARCH-unknown-linux-musl"
curl -fsSLO "$BASE/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS
sudo install -m 0755 "zigbee-bridge-v$VERSION-$ARCH-unknown-linux-musl" /usr/local/bin/zigbee-bridge
zigbee-bridge --version
```

**2. Check that the coordinator answers.** Use the stable name under
`/dev/serial/by-id/` rather than `/dev/ttyUSB0`, which can change between
boots.

```sh
ls /dev/serial/by-id/
sudo zigbee-bridge probe /dev/serial/by-id/usb-...
```

It prints the firmware it found, for example `{"kind":"ezsp","firmware":"7.4.4.0",...}`.
If it says the firmware is too old, [update it](https://oss.pulzr.net/zigbee-bridge/hardware.html#flashing-firmware).

**3. Write a minimal configuration** to `/etc/zigbee-bridge/config.toml`.
Only `serial.port` is required; everything else has a default.

<!-- quickstart:config -->
```toml
[serial]
port = "/dev/serial/by-id/usb-..."   # the path from step 2

[mqtt]
host = "localhost"                   # your broker
```

**4. Run it.** On first start it forms a new network with a random network
key, which it keeps in `/var/lib/zigbee-bridge`.

<!-- quickstart:run -->
```sh
sudo zigbee-bridge --config /etc/zigbee-bridge/config.toml
```

**5. Pair a device.** Open the network for two minutes, then reset the
device or hold its pairing button.

<!-- quickstart:pair -->
```sh
mosquitto_pub -h localhost -t zigbee/bridge/request/permit_join -m '{"time": 120}'
```

Within a minute the device appears in Home Assistant under
**Settings > Devices & services > MQTT**. To watch everything the bridge
publishes:

```sh
mosquitto_sub -h localhost -t 'zigbee/#' -v
```

No coordinator yet? The [getting started guide](https://oss.pulzr.net/zigbee-bridge/getting-started.html)
runs the same steps against the simulator. To run the bridge as a service,
see [installation](#installation).

## Installation

- **Release binaries.** Each [release](https://github.com/pulzar-net/zigbee-bridge/releases)
  has static Linux binaries for `x86_64`, `aarch64` (Raspberry Pi 3 and newer
  with a 64-bit OS) and `mipsel` (MT7621 and MT7628 routers), with a
  `SHA256SUMS` file. They have no runtime dependencies.
- **From crates.io**, with Rust 1.89 or newer:
  `cargo install --locked zigbee-bridge`
- **systemd**: a hardened unit is in [`contrib/systemd/`](contrib/systemd/zigbee-bridge.service).
- **OpenWrt**: a procd init script is in [`contrib/openwrt/`](contrib/openwrt/zigbee-bridge.init).

The [installation guide](https://oss.pulzr.net/zigbee-bridge/installation.html)
covers each one step by step.

## Configuration

The bridge reads one TOML file, `/etc/zigbee-bridge/config.toml` by default
(`--config FILE` to change it). It has these tables:

| Table | What it sets |
|---|---|
| `[serial]` | the coordinator's port (serial device or `tcp://host:port`), baud rate, flow control, family |
| `[mqtt]` | broker, credentials (password from a file), client id, base topic |
| `[homeassistant]` | discovery on or off, discovery prefix, Home Assistant's status topic |
| `[network]` | channel and transmit power for a new network; whether to replace a foreign one |
| `[join]` | install-code-only joining, whether pre-Zigbee 3.0 devices may stay |
| `[data]` | the data directory: network keys, device store, backup |
| `[ota]` | where device firmware images are kept |

Unknown keys are an error, so a typo stops the bridge at startup. Every key,
its type and default are in the
[configuration reference](https://oss.pulzr.net/zigbee-bridge/config-reference.html),
which is generated from the code. [`config.example.toml`](config.example.toml)
shows them all. Log verbosity comes from `RUST_LOG` (default `info`).

## MQTT API

Everything lives under a base topic, `zigbee` by default. Payloads are JSON.

| Topic | Direction | What |
|---|---|---|
| `zigbee/bridge/state` | out, retained | `{"state":"online"}` or `{"state":"offline"}` (also the last will) |
| `zigbee/bridge/info` | out, retained | version, coordinator, channel and PAN ids, join state |
| `zigbee/bridge/devices` | out, retained | every device with its interview, endpoints and clusters |
| `zigbee/bridge/event` | out | joins, announces, interviews, leaves, removals, join window, channel and firmware update progress |
| `zigbee/bridge/request/<name>` | in | `permit_join`, `remove`, `rename`, `install_code`, `interview`, `backup`, `topology`, `scan_neighbors`, `energy_scan`, `devices`, `change_channel`, `ota_check`, `ota_update` |
| `zigbee/bridge/response/<name>` | out | `{"status":"ok","data":{...}}` or `{"status":"error","error":"..."}` |
| `zigbee/<device>` | out, retained | the device's state, for example `{"temperature":21.5,"humidity":45.2,"linkquality":160}` |
| `zigbee/<device>/set` | in | `{"state":"ON"}`, `{"brightness":128,"transition":1}`, `{"color_temp":300}` |
| `zigbee/<device>/report` | out | every attribute report, by cluster and attribute name |
| `zigbee/<device>/action` | out | commands the device sends, such as button presses |

`<device>` is the name you gave the device with the `rename` request, or its
IEEE address (`0x00124b0001020304`). Network keys never appear on MQTT. The
[MQTT reference](https://oss.pulzr.net/zigbee-bridge/mqtt.html) has
every payload with examples.

## Use it as a library

The bridge is a thin layer over library crates you can use on their own.

| Crate | What it is | |
|---|---|---|
| [`zigbee-host`](crates/zigbee-host) | the coordinator host: formation, trust center, interview, reporting, device store, backup and restore, topology, probe  [crates.io](https://crates.io/crates/zigbee-host), [docs.rs](https://docs.rs/zigbee-host) |
| [`zigbee-zcl`](crates/zigbee-zcl) | the Zigbee Cluster Library: data types, frames, global commands, cluster definitions  [crates.io](https://crates.io/crates/zigbee-zcl), [docs.rs](https://docs.rs/zigbee-zcl) |
| [`zigbee-zdo`](crates/zigbee-zdo) | Zigbee Device Object frames: discovery, binding, network management  [crates.io](https://crates.io/crates/zigbee-zdo), [docs.rs](https://docs.rs/zigbee-zdo) |
| [`zigbee-ezsp`](crates/zigbee-ezsp) | Silicon Labs EZSP frames and an async client  [crates.io](https://crates.io/crates/zigbee-ezsp), [docs.rs](https://docs.rs/zigbee-ezsp) |
| [`zigbee-ash`](crates/zigbee-ash) | the ASHv2 serial link that carries EZSP, as a sans-IO state machine  [crates.io](https://crates.io/crates/zigbee-ash), [docs.rs](https://docs.rs/zigbee-ash) |
| [`zigbee-znp`](crates/zigbee-znp) | Texas Instruments Z-Stack ZNP frames and an async client  [crates.io](https://crates.io/crates/zigbee-znp), [docs.rs](https://docs.rs/zigbee-znp) |
| [`zigbee-deconz`](crates/zigbee-deconz) | dresden elektronik deCONZ serial frames and an async client  [crates.io](https://crates.io/crates/zigbee-deconz), [docs.rs](https://docs.rs/zigbee-deconz) |
| [`zigbee-primitives`](crates/zigbee-primitives) | addresses, keys and a bounds-checked wire reader shared by all crates  [crates.io](https://crates.io/crates/zigbee-primitives), [docs.rs](https://docs.rs/zigbee-primitives) |
| [`zigbee-sim`](crates/zigbee-sim) | a coordinator simulator with simulated devices, for tests without hardware  [crates.io](https://crates.io/crates/zigbee-sim), [docs.rs](https://docs.rs/zigbee-sim) |
| [`zigbee-bridge`](crates/zigbee-bridge) | the MQTT bridge, as a binary and a library  [crates.io](https://crates.io/crates/zigbee-bridge), [docs.rs](https://docs.rs/zigbee-bridge) |

Each is on [crates.io](https://crates.io/search?q=zigbee-host) with its API
on [docs.rs](https://docs.rs/zigbee-host). For the example below:

```sh
cargo add zigbee-host zigbee-zcl
cargo add tokio --features macros,rt-multi-thread
```

This example starts a network, lets a device join, and reads its
temperature. It runs here against the simulator; on hardware, open the port
with `zigbee_host::port::open("/dev/ttyUSB0", 115_200, FlowControl::None)`
instead.

```rust
use zigbee_host::probe::NcpKind;
use zigbee_host::{random_network, AnyAdapter, Host, HostConfig, HostEvent, StartMode};
use zigbee_zcl::{attr, cluster_id};
# use zigbee_sim::{SimConfig, SimDevice, Simulator};
# use zigbee_primitives::Ieee;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
#   let sim = Simulator::new(SimConfig {
#       devices: vec![SimDevice::temperature_humidity_sensor(Ieee(0x00124b0000000001))],
#       ..SimConfig::default()
#   });
#   let (port, ncp) = tokio::io::duplex(4096);
#   tokio::spawn(async move { sim.serve(ncp).await });
    let (adapter, adapter_events) = AnyAdapter::connect(port, NcpKind::Ezsp).await?;
    let mode = StartMode::Normal { settings: random_network(15)?, replace_other: false };
    let (host, mut events) = Host::start(adapter, adapter_events, mode, HostConfig::default()).await?;

    // Open the network and wait for a device to join and finish its interview.
    host.permit_join(60, None).await?;
    let device = loop {
        match events.recv().await {
            Some(HostEvent::InterviewDone { device }) => break device,
            Some(_) => continue,
            None => return Err("the host stopped".into()),
        }
    };
    println!("{} joined: {:?} {:?}", device.ieee, device.manufacturer, device.model);

    // Read its temperature, if it has a thermometer.
    let cluster = cluster_id::TEMPERATURE_MEASUREMENT;
    if let Some(endpoint) = device.endpoint_with(cluster) {
        let value = attr::temperature_measurement::MEASURED_VALUE;
        let records = host.read_attributes(device.ieee, endpoint.endpoint, cluster, &[value]).await?;
        println!("{records:?}");
    }
    host.shutdown().await?;
    Ok(())
}
```

The [library guide](https://oss.pulzr.net/zigbee-bridge/library/index.html)
covers each crate, and the [API docs](https://oss.pulzr.net/zigbee-bridge/api/zigbee_host/index.html)
cover every item.

## Architecture

```text
 Home Assistant, Node-RED, scripts
                │  MQTT (JSON, Home Assistant discovery)
┌───────────────┴───────────────────────────────────────┐
│ zigbee-bridge   topics, requests, discovery, state     │
├────────────────────────────────────────────────────────┤
│ zigbee-host     formation, trust center, interview,    │
│                 reporting, device store, backup,       │
│                 topology, probe                        │
├──────────────┬──────────────┬──────────────────────────┤
│ zigbee-zcl   │ zigbee-zdo   │ zigbee-primitives        │
├──────────────┴─┬────────────┴─┬────────────────────────┤
│ zigbee-ezsp     │ zigbee-znp   │ zigbee-deconz          │
│ zigbee-ash      │              │                        │
└────────┬────────┴──────┬───────┴────────┬───────────────┘
         │        serial or TCP           │
  Silicon Labs NCP   TI Z-Stack NCP   ConBee, RaspBee   (or zigbee-sim)
         │               │                │
            Zigbee mesh: routers, end devices
```

The coordinator's firmware runs the radio, MAC, network layer and routing.
Everything above it, including the trust center, runs in the host. The
[architecture page](https://oss.pulzr.net/zigbee-bridge/architecture.html)
explains the layers, the event flow and the design rules (no `unsafe`, no
panics on radio input, bounded memory).

## Compared with other projects

| | zigbee-bridge | [Zigbee2MQTT](https://www.zigbee2mqtt.io) | [ZHA](https://www.home-assistant.io/integrations/zha/) |
|---|---|---|---|
| Language and runtime | Rust, one static binary | Node.js | Python, inside Home Assistant |
| Device support | standard clusters only, so far | over 4,000 devices with per-device converters | thousands, with quirks |
| Coordinators | EmberZNet, Z-Stack, deCONZ | EmberZNet, Z-Stack, deCONZ, ZiGate, others | EmberZNet, Z-Stack, deCONZ, ZiGate, XBee |
| Interface | MQTT and Home Assistant discovery | MQTT, discovery, web frontend | Home Assistant UI |
| Small hardware | runs on 128 MB MIPS routers | needs a Node.js capable host | needs Home Assistant |
| Embeddable library | yes, Rust crates | zigbee-herdsman (TypeScript) | zigpy (Python) |
| Maturity | 0.x, not yet hardware-tested | mature, very widely used | mature, very widely used |

**Pick Zigbee2MQTT or ZHA** if you want a large and varied device collection
to work today, a web frontend, or automatic device firmware updates: they are mature and their
device libraries are the work of thousands of contributors. **Pick
zigbee-bridge** if you need Zigbee on hardware too small for Node.js or
Python, a single static binary with no runtime, or a Zigbee stack to embed
in Rust software, and your devices use the standard clusters. The topic
scheme is modeled on Zigbee2MQTT's, so tools written for one need little
change for the other, but the two are not interchangeable.

## Roadmap

- **Next:** manufacturer-specific clusters and device quirks, groups and
  scenes, binding over MQTT.

Questions go in [Discussions](https://github.com/pulzar-net/zigbee-bridge/discussions); bugs and requests in [issues](https://github.com/pulzar-net/zigbee-bridge/issues).

## Contributing

Contributions are welcome, from hardware reports to code. The whole stack
can be developed and tested without hardware, using the simulator. See
[CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test, fuzz and send a
change, and [the code of conduct](CODE_OF_CONDUCT.md).

## Security

Please report vulnerabilities privately through
[GitHub private vulnerability reporting](https://github.com/pulzar-net/zigbee-bridge/security/advisories/new),
never in a public issue. See [SECURITY.md](SECURITY.md) for what to include
and how fast we respond, and the
[security model](https://oss.pulzr.net/zigbee-bridge/security.html)
for what the bridge protects.

## License

MIT. See [LICENSE](LICENSE).

The implementation follows the Zigbee specification and Silicon Labs' and
Texas Instruments' public protocol documentation. zigbee-herdsman (MIT) was
studied as a reference; no code was copied from any project.

## Maintained by Pulzr

zigbee-bridge is maintained by [Pulzr](https://pulzr.net), which uses it in
its own gateways. It is developed in the open, for anyone to use.
