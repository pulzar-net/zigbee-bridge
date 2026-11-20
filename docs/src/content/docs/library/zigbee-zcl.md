---
title: "zigbee-zcl"
description: "The Zigbee Cluster Library: data types, frames, global commands and cluster definitions."
---

The Zigbee Cluster Library: data types, frames, global commands and cluster
definitions. No I/O. [crates.io](https://crates.io/crates/zigbee-zcl), [docs.rs](https://docs.rs/zigbee-zcl), [API docs for `main`](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_zcl/index.html).

| Module | What it has |
|---|---|
| `types` | the ZCL data types (`DataType`) |
| `value` | `Value`: a self-describing attribute value, decoded with bounds |
| `frame` | `Frame` and its header: direction, manufacturer code, sequence number, command |
| `foundation` | `Global`: read and write attributes, configure reporting, reports, default response, discovery |
| `defs` | cluster, attribute and command definitions, and `cluster(id)` to look one up |
| `status` | ZCL status codes |

The cluster definitions are generated at build time from
[`data/clusters.toml`](https://github.com/pulzar-net/zigbee-bridge/blob/main/crates/zigbee-zcl/data/clusters.toml):
names, ids, attribute types, default reporting intervals and command
fields. Adding a cluster or attribute is a change to that file, not to code.
Constants for ids are generated too: `cluster_id::TEMPERATURE_MEASUREMENT`,
`attr::temperature_measurement::MEASURED_VALUE`.

Feature `serde` (default) adds `Serialize` for values: numbers, strings,
lists, and `null` for the ZCL "invalid" value of a type.
