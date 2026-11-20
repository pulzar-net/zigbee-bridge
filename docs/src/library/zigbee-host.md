# zigbee-host

The coordinator host: the application that runs a Zigbee network on top of
an NCP. [API docs](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_host/index.html).

## What it does

- **Formation.** Resume the NCP's network when it matches the settings you
  give, form it when the NCP has none, and refuse when the NCP runs a
  different network, unless told to replace it.
- **Trust center.** Zigbee 3.0 unique link keys derived from a random root
  key, install codes, and a join policy (install codes only; whether
  pre-3.0 devices may stay).
- **Interview.** Node and simple descriptors, the Basic cluster, then
  binding and reporting configuration for the standard clusters and IAS zone
  enrollment. Resumable step by step, for sleepy devices.
- **Events.** Reports, commands, joins, leaves and interview progress arrive
  as `HostEvent`s on a channel. Default responses and the Time cluster are
  answered for you.
- **Requests.** Read and write attributes, configure reporting, send cluster
  commands, bind, and send raw ZCL and ZDO frames you built yourself.
- **Device store.** One JSON file, written atomically, only when a device
  record changes.
- **Backup and restore** in the open coordinator backup format.
- **Topology** from last hops, route records and neighbor scans.
- **Probe.** Identify the NCP on a port, read-only and bounded in time.

## The main types

| Type | What it is |
|---|---|
| `Host<A>` | a running network; cheap to clone, every method takes `&self` |
| `Adapter` | the trait an NCP family implements; `EzspAdapter` and `ZnpAdapter` |
| `AnyAdapter` | whichever family is on the port, chosen at run time |
| `StartMode` | `Normal { settings, replace_other }` or `Restore(backup)` |
| `NetworkSettings` | channel, PAN ids, network key; `random_network(channel)` makes one |
| `HostConfig` | store and backup paths, join policy, timeouts |
| `HostEvent` | what happened on the network |
| `Device` | what the host knows about a device: addresses, interview, endpoints |
| `Backup` | a backup in the open format, to and from JSON |

The README's [library example](https://github.com/pulzar-net/zigbee-bridge#use-it-as-a-library)
starts a network, waits for a device and reads an attribute; it runs as a
doctest in CI.

## Opening a port

`zigbee_host::port::open(name, baud, flow)` opens a serial device, or
`tcp://host:port` for a network coordinator or the simulator. Pass the
result to `AnyAdapter::connect` with the family, or let
`probe::probe_any` find the family first.

## Raw frames for your own device library

The host decodes the standard clusters, but it also hands you every ZCL
frame exactly as received (`HostEvent::Frame`, with endpoints, profile,
cluster, link quality and last hop), and sends frames you build
(`Host::zcl_raw`, `Host::zdo_raw`) with its own sequence numbers. That is
the hook for a device library with manufacturer-specific behavior.

## Features

| Feature | Default | What |
|---|---|---|
| `host` | yes | everything above |
| `serial` | yes | open ports by name (serial devices and `tcp://`) |
| `probe` | no | only the probe; with `default-features = false`, a small build for detection tools |

```toml
zigbee-host = { git = "https://github.com/pulzar-net/zigbee-bridge", tag = "v0.1.0", default-features = false, features = ["probe", "serial"] }
```
