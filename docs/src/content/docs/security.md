---
title: "Security model"
description: "This page describes what zigbee-bridge protects, what it relies on you for, and what it cannot do."
---

This page describes what zigbee-bridge protects, what it relies on you for,
and what it cannot do. To report a vulnerability, see
[SECURITY.md](https://github.com/pulzar-net/zigbee-bridge/blob/main/SECURITY.md):
privately, never in a public issue.

## What is at stake

- **The network key.** Anyone with it can decrypt all network traffic and
  inject frames. It lives in the NCP, in `network.json` and in `backup.json`.
- **The trust center root key**, from which every device's unique link key is
  derived. It lives in the NCP and in `backup.json`.
- **Control of devices**: locks, plugs, alarms. Whoever can publish to
  `zigbee/+/set` or `zigbee/bridge/request/#` controls them.

## Threats and defenses

| Threat | Defense |
|---|---|
| Malformed or hostile frames from a nearby radio | Every parser is bounds checked, allocates in proportion to its input, and is fuzzed; no `unsafe` code. Bad frames are dropped. |
| A device joining without permission | Joining is closed unless a `permit_join` request opens it, for at most 254 seconds. `join.install_code_only` admits only devices whose install code you added. |
| A device keeping the well-known join key | Zigbee 3.0 devices get a unique link key after joining. `join.allow_legacy = false` removes devices that never request one. |
| Keys leaking through MQTT or logs | Keys are never published on MQTT. The `backup` request writes to disk and returns only a summary. `Debug` output of key types is redacted, and keys are not logged at `info` or `debug` level. |
| Keys leaking through files | `network.json`, `devices.json` and `backup.json` are written with mode 0600 in a directory with mode 0700, atomically. |
| Losing the network | Backups after every join and leave and hourly; restore onto a new coordinator with the frame counter advanced. |
| Wiping a network by accident | A coordinator with a different network is refused unless `network.replace_other` is set. |

## What you are responsible for

- **The MQTT broker.** The bridge does whatever authorized MQTT clients ask.
  Require authentication on the broker, and use its access control so only
  the bridge and trusted clients can publish to `zigbee/#`. Keep the broker
  off untrusted networks.
- **The data directory.** It holds the network key. Back it up, and protect
  the backups like a password.
- **Trace logs.** At `trace` level the serial crates log raw frames, which
  include key material when a backup is read from the NCP. Do not post trace
  logs publicly without removing those frames.
- **Physical access.** Whoever holds the coordinator holds the keys in its
  memory.

## Current limits

- **No TLS to the MQTT broker yet.** The bridge connects to the broker over
  plain TCP. Run the broker on the same host or a trusted network, or put a
  TLS tunnel in between.
- **Install codes are held in memory only**; they are lost on restart until
  the device joins.
- **The Zigbee protocol's own limits apply.** Zigbee 3.0 security protects
  frames with the network key, which every device on the network holds; a
  compromised device can read network traffic. Devices pairing with the
  well-known key are exposed during the join window: keep it short.
