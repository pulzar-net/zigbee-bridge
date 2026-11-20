---
title: "zigbee-sim"
description: "A Zigbee coordinator simulator with simulated devices, for testing Zigbee hosts without hardware."
---

A Zigbee coordinator simulator with simulated devices, for testing Zigbee
hosts without hardware. [API docs](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_sim/index.html).

`Simulator` plays one coordinator family over any byte stream:

- Silicon Labs EmberZNet: ASH and EZSP, protocol 13 (as EmberZNet 7.4 does)
  or 14 and later;
- Texas Instruments Z-Stack 3 over ZNP, and dresden elektronik deCONZ
  (both since 0.2.0).

It keeps a network the way an NCP keeps its non-volatile memory (keys, PAN
ids, frame counter, link keys, a written IEEE address), so restarts,
backups and restores behave as on hardware. Simulated devices
(`SimDevice::temperature_humidity_sensor`, `plug`, `contact_sensor`, or
your own endpoints and attributes) join when the network opens, answer
interviews, report, and accept commands. `set_attribute` changes a value, and
the device reports it if reporting was configured for it, as the interview
does.

In tests, connect the host to the simulator with `tokio::io::duplex`; this
is what the README's library example and the integration tests do. To run a
bridge against it, serve it over TCP:

```sh
cargo run -p zigbee-sim --bin zigbee-ncp-sim -- --listen 127.0.0.1:9999 --devices th,plug,door --report-every 10
```

`zigbee-ncp-sim` options: `--listen ADDR`, `--ncp ezsp|znp|deconz`,
`--protocol 13..19` (EZSP version), `--devices` (a comma-separated list of
`th`, `plug`, `door`), and `--report-every SECONDS`.

The simulator is a test tool. It models what a host sees, not radio
propagation, and it is not a reference implementation of the NCP firmware.
