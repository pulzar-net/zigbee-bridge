---
title: "zigbee-bridge"
description: "zigbee-bridge is a Zigbee coordinator stack and a Zigbee to MQTT bridge, written in Rust."
---

zigbee-bridge is a Zigbee coordinator stack and a Zigbee to MQTT bridge,
written in Rust. It runs a Zigbee network through a USB or network
coordinator, interviews the devices that join, and publishes their readings
to MQTT, with Home Assistant discovery. It is a single static binary with no
runtime, small enough for routers and gateways, and a set of library crates
for anyone building Zigbee software in Rust.

This site is the user guide and the reference. The
[README](https://github.com/pulzar-net/zigbee-bridge#readme) is the
two-minute overview; the [API docs](https://oss.pulzr.net/zigbee-bridge/api/zigbee_host/index.html)
cover every type and function.

## Where to start

| You want to | Read |
|---|---|
| try it without hardware | [Getting started](/zigbee-bridge/getting-started.html) |
| run it with a coordinator | [Installation](/zigbee-bridge/installation.html), then [Hardware and firmware](/zigbee-bridge/hardware.html) |
| connect it to Home Assistant | [Home Assistant](/zigbee-bridge/home-assistant.html) |
| script it or build on its topics | [MQTT API](/zigbee-bridge/mqtt.html) |
| change a setting | [Configuration](/zigbee-bridge/configuration.html) and the [reference](/zigbee-bridge/config-reference.html) |
| move to a new coordinator | [Backup and restore](/zigbee-bridge/backup.html) |
| embed the stack in Rust | [Library overview](/zigbee-bridge/library.html) |
| understand how it works | [Architecture](/zigbee-bridge/architecture.html) |
| fix a problem | [Troubleshooting and FAQ](/zigbee-bridge/troubleshooting.html) |

## Status

zigbee-bridge is at 0.x. Version 0.3.0 supports Silicon Labs EmberZNet,
Texas Instruments Z-Stack and dresden elektronik ConBee/RaspBee
coordinators. Everything is tested in CI against a coordinator simulator,
but **no release has been tested on real hardware yet**. Pages that
describe something only real hardware can confirm say so.

Features added after the first release are marked with the version that
brought them ("since 0.2.0"). The [release notes](https://github.com/pulzar-net/zigbee-bridge/releases) list what each
release contains.

## Terms used in these docs

- **Coordinator**: the USB stick or network device with the Zigbee radio. Its
  firmware is the **network co-processor (NCP)**: it runs the radio, the MAC
  and network layers, and routing.
- **Host**: the software that drives the NCP over a serial link. That is
  zigbee-bridge: the trust center, device interviews and the application
  layer run here.
- **Device**: anything that joined the network: a router (mains powered,
  relays traffic) or an end device (often a battery sensor).
- **IEEE address**: a device's permanent 64-bit address, written
  `0x00124b0001020304`. The **network address** is the 16-bit address it gets
  when it joins, and can change.
- **Interview**: what the bridge does after a device joins: it reads what the
  device is and what clusters it has, then sets up reporting.
- **Cluster**: a group of attributes and commands in the Zigbee Cluster
  Library (ZCL), such as On/Off or Temperature Measurement.
