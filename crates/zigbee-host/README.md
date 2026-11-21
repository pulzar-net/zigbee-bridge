# zigbee-host

A Zigbee coordinator host.

The NCP runs the mesh; this crate runs the coordinator's application:

- **Formation** with caller-supplied keys (`NetworkSettings`): resume
  the NCP's network when it matches, form it when the NCP has none, and
  refuse (never silently wipe) when the NCP runs another network.
- **Trust center**: Zigbee 3.0 unique link keys (hashed from a random
  root key), install codes (`install_code`), and a join policy that can
  require install codes or turn away pre-3.0 devices.
- **Interview** (`interview`): node and simple descriptors, the Basic
  cluster, then binding and reporting configuration from the cluster
  library's defaults and IAS zone enrollment; resumable per step.
- **Reports and commands** from devices as `HostEvent`s, with default
  responses and the Time cluster answered.
- **Device store** (`store`): one JSON file, written atomically and only
  when a device record changes, batched.
- **Backup and restore** (`backup`) in the open coordinator backup
  format: network key and frame counter, PAN ids, channel, coordinator
  IEEE address and trust center link keys, restorable onto a new
  coordinator of either family.
- **Topology** (`topology`) from last-hop link quality, route records
  and neighbor-table scans.
- **Probe** (`probe`): what NCP is on a port, read-only and bounded.

The NCP sits behind the `Adapter` trait: `EzspAdapter` for Silicon
Labs EmberZNet 7.4 and 8.x, `ZnpAdapter` for Texas Instruments
Z-Stack 3, `DeconzAdapter` for dresden elektronik ConBee and RaspBee,
and `AnyAdapter` to choose at run time.

Features: `host` (everything above), `probe` with `serial` alone for a
small detection tool, `serial` to open ports by name.

Part of [zigbee-bridge](https://github.com/pulzar-net/zigbee-bridge), maintained by
[Pulzar](https://pulzr.net). See the [user guide](https://oss.pulzr.net/zigbee-bridge/), the
[library guide](https://oss.pulzr.net/zigbee-bridge/library/index.html) and the
[API documentation](https://oss.pulzr.net/zigbee-bridge/api/zigbee_host/index.html).

License: MIT.
