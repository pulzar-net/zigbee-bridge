---
title: "zigbee-deconz"
description: "dresden elektronik's deCONZ serial protocol, which ConBee II, ConBee III and RaspBee II coordinator firmware speaks."
---

dresden elektronik's deCONZ serial protocol, which ConBee II, ConBee III and
RaspBee II coordinator firmware speaks. Since 0.2.0.
[crates.io](https://crates.io/crates/zigbee-deconz), [docs.rs](https://docs.rs/zigbee-deconz), [API docs for `main`](https://oss.pulzr.net/zigbee-bridge/api/zigbee_deconz/index.html).

- `frame`: SLIP framing, the frame header and checksum.
- `message`: requests and responses, typed in both directions.
- `client` (feature `client`, default): an async client.

zigbee-host's `DeconzAdapter` runs a network on it: formation from network
parameters, the firmware's queues read on every device state, the firmware
watchdog refreshed, backup and restore, and energy scans. Written from
dresden elektronik's public serial protocol documentation.
