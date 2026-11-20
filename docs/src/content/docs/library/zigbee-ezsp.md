---
title: "zigbee-ezsp and zigbee-ash"
description: "The two protocols between a host and a Silicon Labs EmberZNet NCP."
---

The two protocols between a host and a Silicon Labs EmberZNet NCP.
[zigbee-ezsp API](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_ezsp/index.html),
[zigbee-ash API](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_ash/index.html).

## zigbee-ash

ASHv2, the Asynchronous Serial Host protocol that carries EZSP over a UART
(Silicon Labs UG101).

- `frame`: CRC-CCITT, byte stuffing, data randomization, and a bounded
  stream `Decoder`. Checked against UG101's byte-level examples.
- `conn`: the link as a sans-IO state machine: RST and RSTACK, frame
  numbering, piggybacked and explicit ACKs, NAK on loss or corruption,
  retransmission, and the adaptive ACK timer.
- `transport` (feature `tokio`, default): a driver that runs the state
  machine over any `AsyncRead + AsyncWrite`.

## zigbee-ezsp

EZSP, the EmberZNet Serial Protocol (Silicon Labs UG100), for protocol
versions 13 to 19 (EmberZNet 7.4 to 8.x) and 8 (EmberZNet 6.7 to 6.10, the
EM35x's last firmware).

- `frame`: the legacy and extended frame headers.
- `command`: typed commands and responses, in both directions, with the
  layouts that changed at protocol 14.
- `callback`: typed callbacks from the NCP.
- `client` (feature `client`, default): `Ezsp`, an async client that
  resets the link, negotiates the protocol version, matches responses to
  commands, and delivers callbacks as events. `call` sends a command and
  returns its response.

Use these crates directly to build EmberZNet tools (a firmware inspector, a
token reader) without the Zigbee host. For a network, use zigbee-host.
