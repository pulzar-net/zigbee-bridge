---
title: "zigbee-zdo"
description: "Zigbee Device Object frames (Zigbee specification section 2.4): device and service discovery, binding, and network management."
---

Zigbee Device Object frames (Zigbee specification section 2.4): device and
service discovery, binding, and network management. No I/O.
[crates.io](https://crates.io/crates/zigbee-zdo), [docs.rs](https://docs.rs/zigbee-zdo), [API docs for `main`](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_zdo/index.html).

`Message` is one ZDO frame: a transaction sequence number and a typed
`Body`. Requests and responses both encode and decode, so a host and a
simulated device share one definition. ZDO clusters the crate does not
decode pass through as `Body::Other`.

Decoded: network and IEEE address, node, power and simple descriptors,
active endpoints, match descriptor, device announce, bind and unbind, and the
management requests (neighbor table, routing table, binding table, leave,
permit joining, network update), each with its response.

Feature `serde` (default) adds `Serialize` for descriptors and table
entries.
