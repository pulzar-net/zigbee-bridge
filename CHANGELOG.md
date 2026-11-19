# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crates
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `zigbee-types`: IEEE addresses, extended PAN ids, redacted keys and a
  bounds-checked wire reader.
- `zigbee-ash`: ASHv2 framing (CRC, byte stuffing, randomization) and the
  link layer as a sans-IO state machine with a tokio driver.
- `zigbee-ezsp`: EZSP frames for protocol versions 13 to 19 (EmberZNet 7.4
  to 8.x) and an async client.
- `zigbee-zcl`: ZCL data types, values, frames, global commands, and cluster
  definitions generated from `data/clusters.toml`.
- `zigbee-zdo`: ZDO requests and responses.
