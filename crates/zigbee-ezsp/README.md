# zigbee-ezsp

EZSP, the EmberZNet Serial Protocol between a host and a Silicon Labs
Zigbee network co-processor (Silicon Labs UG100), for EmberZNet 7.4 and
8.x (protocol versions 13 to 19).

- `frame`: the legacy and extended frame headers.
- `command`: typed commands and their responses, encoding and decoding
  in both directions, with the layouts that changed at protocol 14.
- `callback`: typed callbacks.
- `types`: EZSP structures and `types::Status`, which spans the 8-bit
  `EmberStatus` (protocol 13) and 32-bit `sl_status_t` (14+) code spaces.
- `ids`: frame, configuration, policy and value ids.
- `client` (feature `client`): an async client over an ASH link that
  negotiates the version, serializes commands and delivers callbacks.

Every parser is bounds-checked and allocation-bounded by its input.

Part of [zigbee-bridge](https://github.com/pulzar-net/zigbee-bridge), maintained by
[Pulzr](https://pulzr.net). See the [user guide](https://oss.pulzr.net/zigbee-bridge/), the
[library guide](https://oss.pulzr.net/zigbee-bridge/library/index.html) and the
[API documentation](https://oss.pulzr.net/zigbee-bridge/api/zigbee_ezsp/index.html).

License: MIT.
