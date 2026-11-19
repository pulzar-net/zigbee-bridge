# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crates
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0]

The first milestone: Silicon Labs EmberZNet coordinators.

### Added

- `zigbee-types`: IEEE addresses, extended PAN ids, keys redacted from
  `Debug`, and a bounds-checked wire reader.
- `zigbee-ash`: ASHv2 framing (CRC, byte stuffing, randomization), checked
  against UG101's byte examples, and the link layer as a sans-IO state
  machine with a tokio driver.
- `zigbee-ezsp`: EZSP frames for protocol versions 13 to 19 (EmberZNet 7.4
  to 8.x) in both directions, and an async client.
- `zigbee-zcl`: ZCL data types, values, frames, global commands, and 41
  clusters generated from `data/clusters.toml`.
- `zigbee-zdo`: ZDO requests and responses.
- `zigbee-host`: network formation with caller-supplied keys, Zigbee 3.0
  trust center with unique link keys and install codes, a resumable
  interview, reporting configuration and IAS zone enrollment, the device
  store, backups in the open coordinator backup format and restore onto new
  hardware, topology from last hops, route records and neighbor scans, and
  `probe`.
- `zigbee-sim`: an EZSP NCP simulator with simulated sensors, plugs and
  contact sensors, and `zigbee-ncp-sim`.
- `zigbee-bridge`: the standalone MQTT bridge with Home Assistant discovery.
