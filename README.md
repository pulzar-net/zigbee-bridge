# zigbee-bridge

A Zigbee host stack and MQTT bridge in Rust, maintained by
[Pulzar](https://pulzr.net). MIT licensed.

The coordinator dongle (the network co-processor, NCP) runs the mesh: radio,
MAC, network layer and routing. This project runs everything above it:
forming the network, the trust center, interviewing devices, the Zigbee
Cluster Library, reporting, backups and topology. It is a set of library
crates you can embed, and a standalone bridge that speaks MQTT with Home
Assistant discovery and a plain JSON topic scheme.

**Status: 0.1.0, first milestone, not yet tested on hardware.** Silicon Labs
EmberZNet coordinators only. Everything is exercised in CI against the EZSP
NCP simulator in this repository; bench testing on real dongles is next.

## Coordinators

| Family | Protocol | State |
|---|---|---|
| Silicon Labs EmberZNet 7.4 and 8.x | EZSP 13 to 19 over ASHv2 | supported (simulator-tested) |
| Silicon Labs EmberZNet 7.3 and older | EZSP 12 and older | refused with a "please update the firmware" message; `probe` still identifies it |
| Texas Instruments Z-Stack 3.x | MT/ZNP | next milestone |
| dresden elektronik ConBee II/III | deCONZ serial | next milestone |

Any EFR32MG21 or MG24 USB or network coordinator with EmberZNet 7.4 or newer
NCP firmware should work, for example the Home Assistant Connect ZBT-1, the
Sonoff ZBDongle-E (after a firmware update), or an SMLIGHT SLZB-06M
(over `tcp://`).

## Crates

| Crate | What it does |
|---|---|
| `zigbee-types` | IEEE addresses, PAN ids, keys (never printed by `Debug`), a bounds-checked wire reader |
| `zigbee-ash` | ASHv2: CRC, byte stuffing, randomization, and the link as a sans-IO state machine (window, ACK/NAK, retransmission, adaptive ACK timer) with a tokio driver |
| `zigbee-ezsp` | EZSP frames for protocol 13 to 19, typed commands, responses and callbacks in both directions, and an async client that negotiates the version |
| `zigbee-zcl` | ZCL data types, values, frames, global commands, and clusters generated at build time from [`data/clusters.toml`](crates/zigbee-zcl/data/clusters.toml) |
| `zigbee-zdo` | ZDO discovery, binding and management frames |
| `zigbee-host` | the coordinator host: formation, trust center, interview, reporting, device store, backup and restore, topology, `probe` |
| `zigbee-sim` | an EZSP NCP simulator with simulated devices, and `zigbee-ncp-sim` to serve it over TCP |
| `zigbee-bridge` | the standalone MQTT bridge |

`zigbee-host` with `default-features = false, features = ["probe", "serial"]`
is only the probe, for detection tools with a small flash budget.

## Running the bridge

Each [release](https://github.com/pulzar-net/zigbee-bridge/releases) has
static Linux binaries for `aarch64`, `mipsel` and `x86_64`, with SHA-256
sums. Or build it with `cargo build --release -p zigbee-bridge`.

```sh
cp config.example.toml /etc/zigbee-bridge/config.toml   # set serial.port
zigbee-bridge --config /etc/zigbee-bridge/config.toml
```

On first start the bridge writes `network.json` (random PAN ids and network
key, mode 0600) to the data directory and forms that network. A platform that
manages keys itself writes `network.json` before the first start. If the
coordinator already runs a different network, the bridge refuses to start
rather than wipe it (set `network.replace_other` to replace it).

Open the network for joining:

```sh
mosquitto_pub -t zigbee/bridge/request/permit_join -m '{"time": 180}'
```

### Topics

Under the base topic (`zigbee` by default):

| Topic | Retained | What |
|---|---|---|
| `bridge/state` | yes | `{"state":"online"}` or `"offline"` (the last will) |
| `bridge/info` | yes | version, coordinator, network, join state |
| `bridge/devices` | yes | every device and its interview |
| `bridge/event` | no | `device_joined`, `device_announce`, `device_interview`, `device_leave`, `device_removed`, `permit_join` |
| `bridge/request/<name>` | no | `permit_join` `{time, device?}`, `remove` `{device, force?}`, `rename` `{from, to}`, `install_code` `{ieee_address, code}`, `interview` `{device}`, `backup`, `topology`, `scan_neighbors` `{device?}`, `energy_scan`, `devices` |
| `bridge/response/<name>` | no | `{"status":"ok","data":...}` or `{"status":"error","error":...}`, with the request's `transaction` echoed |
| `<device>` | configurable | standard readings (`temperature`, `humidity`, `battery`, `state`, `contact`, ...) and `linkquality` |
| `<device>/set` | no | `{"state":"ON"}`, `{"brightness":128,"transition":1}`, `{"color_temp":300}` |
| `<device>/report` | no | every attribute report, by cluster and attribute name, for devices the bridge has no field for |
| `<device>/action` | no | cluster commands the device sends (buttons, alarms) |

`<device>` is the device's name, or its IEEE address as `0x00124b0001020304`.
Readings come from the ZCL standard clusters, so an unknown device still
reports what the standard says it has. Network keys never appear on MQTT.

Home Assistant discovers every device under `homeassistant/` once its
interview finishes, and again when Home Assistant restarts.

### Backup and restore

The bridge keeps `backup.json` in the data directory (mode 0600), taken
hourly and after every join or leave, in the open coordinator backup format:
the network key and its frame counter, PAN ids, channel, the coordinator's
IEEE address and the trust center's keys. To replace a dead dongle without
re-pairing devices:

```sh
zigbee-bridge restore /var/lib/zigbee-bridge/backup.json
zigbee-bridge
```

The restore writes the old coordinator's IEEE address to the new dongle
(EmberZNet allows this once), exits with status 75 so a supervisor starts it
again, and on the next start restores the network with the frame counter
advanced past anything the old coordinator sent. A dongle whose IEEE address
was already written is refused, with the reason.

### Probing a port

```sh
zigbee-bridge probe /dev/ttyUSB0
{"kind":"ezsp","firmware":"7.4.4.0","protocol":13,"supported":true,"ieee_address":"0x...","manufacturer":"...","board":"..."}
```

One ASH reset and read-only queries; it never writes and finishes within
three seconds.

## Trying it without hardware

```sh
cargo run -p zigbee-sim --bin zigbee-ncp-sim -- --listen 127.0.0.1:9999 --devices th,plug,door --report-every 10
# in zigbee-bridge's config: [serial] port = "tcp://127.0.0.1:9999"
```

## Security

- Zigbee 3.0 trust center: devices join with the well-known key (or their
  install code key), then receive a unique link key the coordinator derives
  from a random root key that is part of every backup.
- `join.install_code_only` admits only devices whose install code was given
  with the `install_code` request; install codes are held in memory only.
- `join.allow_legacy = false` removes devices that never request a unique
  link key (pre-Zigbee 3.0 devices).
- `network.json`, `backup.json` and `devices.json` are written atomically and
  readable by their owner only. Keys are redacted from `Debug` output.

## Development

```sh
cargo test --workspace            # includes the simulator-driven end-to-end tests
cargo clippy --workspace --all-targets -- -D warnings
cargo +nightly fuzz run ezsp_frames   # see fuzz/ for every target
```

CI runs formatting, lints, every test, a memory budget check with 200
simulated devices (under 16 MB resident), a fuzz run of every frame parser,
and static musl builds for `aarch64` and `mipsel` (under 4 MB) with the test
suite run under qemu. No `unsafe` code anywhere; parsers are bounds-checked
and their allocations are bounded by their input.

The reference implementations studied were zigbee-herdsman (MIT) and
Silicon Labs' public EZSP and ASH documentation (UG100, UG101); no code was
copied from any project.

## License

MIT. See [LICENSE](LICENSE).
