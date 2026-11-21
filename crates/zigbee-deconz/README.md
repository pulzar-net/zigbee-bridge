# zigbee-deconz

dresden elektronik's deCONZ serial protocol, which ConBee, ConBee II,
ConBee III, RaspBee and RaspBee II coordinator firmware speaks.

- `frame`: SLIP framing, the frame header and checksum.
- `message`: requests and responses, typed in both directions.
- `client` (feature `client`): an async client.

Written from dresden elektronik's public serial protocol documentation;
no code was copied from any other implementation.

Part of [zigbee-bridge](https://github.com/pulzar-net/zigbee-bridge), maintained by
[Pulzar](https://pulzr.net). See the [user guide](https://oss.pulzr.net/zigbee-bridge/), the
[library guide](https://oss.pulzr.net/zigbee-bridge/library/index.html) and the
[API documentation](https://oss.pulzr.net/zigbee-bridge/api/zigbee_deconz/index.html).

License: MIT.
