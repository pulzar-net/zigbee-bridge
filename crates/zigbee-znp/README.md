# zigbee-znp

Texas Instruments Z-Stack ZNP: the Monitor and Test (MT) serial protocol
that Z-Stack coordinator firmware speaks, for Z-Stack 3.0.x (CC2530,
CC2531) and 3.x.0 (CC13x2, CC26x2, CC2652).

- `frame`: MT frames and a streaming decoder.
- `message`: the requests a host sends and the indications the NCP
  sends, typed in both directions.
- `client` (feature `client`): an async client.

Written from TI's public MT API documentation; no code was copied from
any other implementation.

Part of [zigbee-bridge](https://github.com/pulzar-net/zigbee-bridge), maintained by
[Pulzar](https://pulzr.net). See the [user guide](https://oss.pulzr.net/zigbee-bridge/), the
[library guide](https://oss.pulzr.net/zigbee-bridge/library/index.html) and the
[API documentation](https://oss.pulzr.net/zigbee-bridge/api/zigbee_znp/index.html).

License: MIT.
