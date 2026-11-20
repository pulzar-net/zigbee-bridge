# Configuration

zigbee-bridge reads one TOML file, `/etc/zigbee-bridge/config.toml` unless
`--config FILE` names another. Only the coordinator's port is required:

```toml
[serial]
port = "/dev/serial/by-id/usb-Nabu_Casa_SkyConnect_v1.0_..."
```

Everything else has a default. Unknown keys and wrong types are errors, so a
typo stops the bridge at startup with the file, line and key, instead of
being silently ignored. The [configuration reference](config-reference.md)
lists every key; it is generated from the code, so it is always current.

A [JSON Schema](config.schema.json) generated from the same code lets an
editor with TOML schema support (Taplo, or VS Code's Even Better TOML)
complete and check the file. Point it at the schema with a first line of:

```toml
#:schema https://pulzar-net.github.io/zigbee-bridge/config.schema.json
```

## The tables

| Table | What it sets |
|---|---|
| `[serial]` | the coordinator's port: a serial device or `tcp://host:port`, baud rate, flow control, and (since 0.2.0) the coordinator family: `auto`, `ezsp`, `znp` or `deconz` |
| `[mqtt]` | broker host and port, user name, a password file, client id, base topic, keep-alive, whether device state is retained |
| `[homeassistant]` | discovery on or off, the discovery prefix, Home Assistant's status topic |
| `[network]` | channel and transmit power of a new network, and whether a different network on the coordinator may be replaced |
| `[join]` | whether only devices with an install code may join, and whether pre-Zigbee 3.0 devices may stay |
| `[data]` | the data directory |
| `[ota]` | where device firmware images are kept (since 0.2.0) |

## A complete example

[`config.example.toml`](https://github.com/pulzar-net/zigbee-bridge/blob/main/config.example.toml)
in the repository sets every key to its default, with comments. CI checks
that it parses.

```toml
{{#include ../../config.example.toml}}
```

## Choosing the port

Use the stable path under `/dev/serial/by-id/` for a USB coordinator: it
names the device, while `/dev/ttyUSB0` and `/dev/ttyACM0` can swap when
another USB serial device is plugged in. For a coordinator on the network
(such as an SMLIGHT SLZB-06M in coordinator mode), use `tcp://address:6638`
with the port its web interface shows.

`baud` must match the firmware. Most EmberZNet and Z-Stack coordinator
firmware runs at 115200, the default. `flow_control` must also match:
Silicon Labs firmware builds named `ncp-uart-hw` use RTS/CTS (`hardware`),
builds named `ncp-uart-sw` use XON/XOFF (`software`). Run
`zigbee-bridge probe PORT --baud N` to check a setting before using it.

## Secrets

The MQTT password is never in the configuration file: `mqtt.password_file`
names a file that holds it. Keep that file readable by the bridge's user
only.

The network key is not in the configuration either. On first start the
bridge generates a random one and writes it, with the PAN ids and channel,
to `network.json` in the data directory (mode 0600):

```json
{
  "channel": 15,
  "pan_id": "0x1a62",
  "extended_pan_id": "dd7c5e3a9b1f4e02",
  "network_key": "0123456789abcdef0123456789abcdef"
}
```

To run a network with keys you manage yourself, write this file before the
first start. After the first start, `network.json` is the source of truth:
changing `network.channel` in the configuration does not move an existing
network.

## The data directory

| File | What it holds | Written |
|---|---|---|
| `network.json` | network key, PAN ids, channel | once, on first start |
| `devices.json` | every device: addresses, names, interview, endpoints | when a device record changes, batched |
| `backup.json` | the latest network backup, in the open coordinator backup format | hourly and after every join or leave |
| `restore-pending.json` | a backup staged by `zigbee-bridge restore` | until the next start restores it |
| `ota/` | device firmware images for `ota_update`, unless `ota.dir` is set | by you |

All of them are written atomically (a temporary file, then a rename) and are
readable by their owner only, because they hold the network key. Back up the
whole directory. [Backup and restore](backup.md) explains moving a network
to a new coordinator.

## Logging

The bridge logs to standard error. Set the level with `RUST_LOG`:

| `RUST_LOG` | What you see |
|---|---|
| unset or `info` | startup, joins, interviews, errors |
| `debug` | every request and report |
| `info,zigbee_ezsp=trace` | every EZSP command and callback |
| `info,zigbee_ash=trace` | every serial frame |

Keys are never logged at any level.
