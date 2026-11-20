---
title: "Architecture"
description: "A Zigbee coordinator is split in two:"
---

## Who does what

A Zigbee coordinator is split in two:

- The **NCP** (network co-processor) is the firmware on the radio stick. It
  runs the radio, the IEEE 802.15.4 MAC, the Zigbee network layer and
  routing, and the APS layer's encryption. It keeps the network's keys and
  counters in its non-volatile memory.
- The **host** is software on the computer. It decides what network to run,
  acts as the trust center that admits devices, interviews them, configures
  them, and turns their messages into something useful.

zigbee-bridge is the host. It talks to the NCP over a serial line (or TCP)
in the NCP's own protocol: EZSP over ASH for Silicon Labs EmberZNet, ZNP for
Texas Instruments Z-Stack.

## Layers

```text
 MQTT clients (Home Assistant, Node-RED, scripts)
        │
 zigbee-bridge ── config, topics, requests, state, discovery
        │  HostEvent (channel)            ▲ Host methods (async)
        ▼                                 │
 zigbee-host ──── formation, trust center, interview, reporting,
        │         device store, backup, topology, probe
        │  AdapterEvent (channel)         ▲ Adapter trait (async)
        ▼                                 │
 EzspAdapter ──── zigbee-ezsp ── zigbee-ash ──┐
 ZnpAdapter  ──── zigbee-znp ─────────────────┤ serial port or TCP
                                              ▼
                                       the NCP (or zigbee-sim)

 zigbee-zcl, zigbee-zdo: frame codecs used by the host, the bridge and the simulator
 zigbee-primitives: addresses, keys and the wire reader used by everything
```

Each layer has one job and talks to its neighbors through a small interface:

- **`Adapter`** is the boundary between the host and an NCP family: start a
  network, send an APS frame, add a key, read a backup, and a stream of
  `AdapterEvent`s (incoming frames, joins, key exchanges, failures). Adding a
  coordinator family means implementing this trait; the host above does not
  change.
- **`Host`** owns the network's state and serializes access to it. Its
  methods are async and take `&self`; it can be cloned and shared between
  tasks. It emits `HostEvent`s on an unbounded channel.
- **The bridge** owns the MQTT session and the mapping between devices and
  topics. It never touches the NCP directly.

## The life of a reading

1. A temperature sensor sends a ZCL Report Attributes frame. The NCP
   decrypts it and passes it to the host as an incoming APS message.
2. zigbee-ash checks the frame's CRC, acknowledges it, and hands the EZSP
   frame up; zigbee-ezsp decodes it into an `incomingMessageHandler`
   callback.
3. The adapter turns the callback into an `AdapterEvent`. zigbee-host looks
   up the device by network address, decodes the ZCL frame with zigbee-zcl,
   records the last-hop link for the topology, sends a default response if
   one is due, and emits `HostEvent::Attributes`.
4. zigbee-bridge publishes the raw report on `<device>/report`, converts the
   standard attributes to fields (`temperature: 21.45`), merges them into the
   device's state, and publishes the state.

Commands go the other way: a `<device>/set` message becomes a
`Host::command` call, which builds the ZCL frame, sends it through the
adapter, and waits for the device's response.

## Design rules

- **Never panic on input.** Every byte from a radio or serial port is parsed
  with bounds checks, and every parser is fuzzed in CI. A bad frame is
  dropped and logged.
- **Bounded resources.** Requests in flight to the NCP are limited so its
  buffers are never exhausted; parsers allocate in proportion to their
  input; the topology keeps a bounded number of links. CI checks that the
  host stays under 16 MB resident with 200 simulated devices.
- **Never lose the network.** Keys are generated once and written before the
  network is formed. A coordinator with another network is never wiped
  unless the configuration says so. Every state file is written atomically.
  Backups are taken after every change to the device list.
- **No `unsafe` code**, forbidden by the workspace lints.
- **Small and static.** Release binaries are statically linked against musl,
  built for size, and must stay under 4 MB; CI fails a build that grows past
  it.
- **Standards over special cases.** Devices are handled through the standard
  clusters. Device-specific behavior belongs in data or in a device library
  built on the raw-frame hooks, not in the host.

## Concurrency

Everything runs on tokio. The ASH link, the EZSP client, the host's event
loop, its store writer and its periodic tasks (backups, topology aging) are
separate tasks connected by channels. The bridge runs one loop that selects
over host events, MQTT messages and the stop signal, so its state needs no
locks.

## Files

The host's device store and backup, and the bridge's network file, are JSON
files in the data directory, written with a temporary file, `fsync` and a
rename, readable by their owner only. See
[the data directory](/zigbee-bridge/configuration.html#the-data-directory).
