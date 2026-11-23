# zigbee-zdo

Zigbee Device Object (ZDO) frames, Zigbee specification section 2.4.

`Message` is one ZDO frame: a transaction sequence number and a typed
`Body`. Requests and responses both parse and encode, so a host and a
simulated device share one definition. Clusters this crate does not
decode pass through as `Body::Other`.

Part of [zigbee-bridge](https://github.com/pulzar-net/zigbee-bridge), maintained by
[Pulzr](https://pulzr.net). See the [user guide](https://oss.pulzr.net/zigbee-bridge/), the
[library guide](https://oss.pulzr.net/zigbee-bridge/library/index.html) and the
[API documentation](https://oss.pulzr.net/zigbee-bridge/api/zigbee_zdo/index.html).

License: MIT.
