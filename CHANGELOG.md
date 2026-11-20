# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crates
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- EZSP protocol 8: EmberZNet 6.7 to 6.10, the last firmware for the EM35x
  (EM357, EM3581) and what the Sonoff ZBDongle-E ships with. The keys go
  through `getKey`, `getKeyTableEntry`, `setKeyTableEntry` and
  `addTransientLinkKey` (new `Command` and `Response` variants and
  `types::KeyStruct`), and `startScan` answers with a one-byte status.
  `ProtocolVersion::sec_man()` says which key API a version uses. Tested on
  an EM357 on EmberZNet 6.7.10 (a Control4 CA-1's onboard radio): probe,
  resuming its network, backup, energy scan and permit join. The simulator
  plays protocol 8 (`--protocol 8`), and the host's simulator suite runs
  against it.

### Fixed

- `probe` waits ASH's full reset time for an RSTACK, with one reset: a
  second reset sent after 1.1 s landed while an EM357 (about 1.5 s to
  answer) was still rebooting and cut its RSTACK off, so it was reported
  as no coordinator. `PROBE_TIMEOUT` is 4 seconds.
- Opening a serial port its last user has just closed waits up to 3
  seconds for the kernel to release it, instead of failing with "Unable
  to acquire exclusive lock". `probe` hit it when it reopened the port for
  the next coordinator family, which hid what the port had answered.

## [0.3.0] - 2026-11-20

The first release on crates.io.

### Changed

- `zigbee-types` is renamed `zigbee-primitives` (crate `zigbee_primitives`),
  since crates.io already has a `zigbee-types`. Replace the dependency and
  the `use` paths; nothing else changes.

### Added

- The crates are ready for crates.io: descriptions, docs.rs links, each
  crate's README as its crates.io page, and packages that carry only the
  sources, README, license and the tests that run from the package. CI dry
  runs `cargo publish --workspace` on every push, and a version tag
  publishes the crates in dependency order.

## [0.2.0] - 2026-11-20

Texas Instruments Z-Stack and dresden elektronik deCONZ coordinators,
channel change, an OTA upgrade server, and backups that restore onto any
coordinator family.

### Added

- `zigbee-znp`: Texas Instruments Z-Stack MT frames, typed requests and
  indications in both directions, and an async client, for Z-Stack 3.x.0
  and 3.0.x.
- `zigbee-host`: `ZnpAdapter` (formation with a random link key seed,
  resume, install codes, raw ZDO both ways, backup and restore with the
  frame counter advanced, energy scan), and `AnyAdapter` to choose the
  family at run time.
- `zigbee-host`: `probe` identifies Z-Stack coordinators (`NcpKind::Znp`,
  `NcpIdentity::product`); `probe_any` tries every family and
  `probe_stream` probes an open stream.
- `zigbee-host`: backups carry Z-Stack's link key seed
  (`stack_specific.zstack.tclk_seed`).
- `zigbee-deconz`: the deCONZ serial protocol (ConBee, RaspBee): SLIP
  framing, typed requests and responses, and an async client.
- `zigbee-host`: `DeconzAdapter` (formation from parameters, the firmware's
  queues read on every device state, the firmware watchdog refreshed,
  backup and restore, energy scan) and `NcpKind::Deconz` in the probe.
- `zigbee-host`: a device first heard of by its announce counts as joined
  (`HostEvent::DeviceJoined`), for coordinators that report no joins.
- `zigbee-sim`: ZNP and deCONZ front-ends (`SimNcp::Znp`, `SimNcp::Deconz`)
  on the same simulated network as EZSP; `zigbee-ncp-sim --ncp`.
- `zigbee-bridge`: `serial.adapter` (`auto`, `ezsp`, `znp`, `deconz`);
  `auto` probes the port.
- `zigbee-host`: `Host::change_channel` moves the network to another
  channel on every family (a network update broadcast with the next update
  id, then the coordinator confirmed on the new channel), with
  `HostEvent::ChannelChanged` and `Adapter::network_info`.
- `zigbee-bridge`: the `change_channel` request (`{"channel": 20}`), the
  `channel_changed` event, `network.update_id` in `bridge/info`; the new
  channel is saved to `network.json`.
- `zigbee-host`: an OTA Upgrade server: `ota` reads OTA files (vendor
  prefixes included) from a directory and serves blocks from disk;
  `Host::ota_check` and `Host::ota_update` notify a device and answer its
  query, image block and page requests and upgrade end;
  `HostEvent::OtaQuery`, `OtaProgress`, `OtaFinished`. Nothing is offered
  without `ota_update`.
- `zigbee-bridge`: `ota_check` and `ota_update` requests (answered when the
  device responds), `ota_available`, `ota_progress` and `ota_finished`
  events, and `[ota] dir` (default `<data.dir>/ota`).
- `zigbee-sim`: an OTA client on the simulated plug (`SimFirmware`).
- `zigbee-host`: a backup restores onto a coordinator of any family: the
  IEEE address, PAN ids, channel, network key and frame counter (advanced)
  always; Z-Stack's link key seed or EmberZNet's root key when restoring
  onto the same family, and the backup's per-device link keys otherwise.
  Backups written by other open-coordinator-backup hosts read as well.
- `zigbee-host`: `Adapter::active_scan` and `Host::active_scan` (other
  networks' beacons: EmberZNet's network-found callbacks, Z-Stack's network
  discovery; deCONZ has none), `Adapter::leave` and `Host::leave` (erase the
  network from the coordinator and forget every device), and
  `Host::set_install_codes` (replace the whole list, checked first).
- `zigbee-ezsp`: the `networkFoundHandler` callback. `zigbee-znp`:
  `ZDO_NWK_DISCOVERY_REQ`, beacon notifications and the discovery confirm.
- `zigbee-sim`: networks in range (`SimConfig::neighbors`).

### Changed

- `zigbee-host`: `AdapterEvent::Message`, `AdapterEvent::RouteRecord` and
  `FrameLink` carry signal strength (and, for route records, the IEEE
  address and link quality) as `Option`, since Z-Stack does not report
  them. `AdapterInfo` gains `reports_key_exchange`. `NcpKind` is
  `#[non_exhaustive]`.
- `zigbee-host`: `Host::network` returns the network by value (it changes
  with the channel).
- `zigbee-sim`: `SimConfig` names the family with `ncp: SimNcp`;
  `SimNetwork` is family-neutral (`channel`, `pan_id`, ... instead of
  EmberZNet's network parameters).

## [0.1.0] - 2026-11-19

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
- `zigbee-host`: `HostEvent::Frame`, every ZCL frame from a device exactly as
  received with how it arrived (endpoints, profile, cluster, group, link
  quality, last hop), for embedders with their own device library.
- `zigbee-host`: `Host::zcl_raw` and `Host::zdo_raw` send caller-built frames,
  write the host's own sequence number, and return the reply as received or
  wait for APS delivery only.
- `zigbee-host`: the interview also reads the ZCL, stack and hardware
  versions.
