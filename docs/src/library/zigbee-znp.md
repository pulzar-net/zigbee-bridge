# zigbee-znp

Texas Instruments Z-Stack's ZNP serial protocol, the Monitor and Test (MT)
API, for Z-Stack 3.0.x (CC2530, CC2531) and 3.x.0 (CC13x2, CC26x2, CC2652).
On `main`, not in 0.1.0. [API docs](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_znp/index.html).

- `frame`: MT frames and a streaming decoder.
- `message`: the requests a host sends and the indications the NCP sends,
  typed in both directions.
- `client` (feature `client`, default): an async client.

zigbee-host's `ZnpAdapter` runs a network on it.
