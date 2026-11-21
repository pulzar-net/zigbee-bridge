---
title: "zigbee-bridge (crate)"
description: "The bridge as a library: the same code as the binary, for programs that want to run it in-process or reuse its parts."
---

The bridge as a library: the same code as the binary, for programs that want
to run it in-process or reuse its parts. [crates.io](https://crates.io/crates/zigbee-bridge), [docs.rs](https://docs.rs/zigbee-bridge), [API docs for `main`](https://oss.pulzr.net/zigbee-bridge/api/zigbee_bridge/index.html).

- `run(config, stop)` opens the coordinator, starts, resumes or restores
  the network, connects to MQTT, and bridges until `stop` resolves.
- `stage_restore` stages a backup, as `zigbee-bridge restore` does.
- `config::Config` is the configuration file; `Config::from_toml` parses
  one with every check the binary applies.
- `exposes` maps standard clusters to state fields and Home Assistant
  discovery messages.
- `bridge` is the topic scheme and request handling.

The [MQTT API](/zigbee-bridge/mqtt.html) and [configuration reference](/zigbee-bridge/config-reference.html)
document the behavior.
