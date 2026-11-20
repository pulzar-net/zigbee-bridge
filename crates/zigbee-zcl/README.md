# zigbee-zcl

The Zigbee Cluster Library.

- `types`: the ZCL data types.
- `value`: self-describing attribute values, with bounded decoding.
- `frame`: the ZCL frame header.
- `foundation`: global commands (read, write, configure reporting,
  reports, default response, discovery).
- `defs`: clusters, attributes and cluster-specific commands, generated
  at build time from `data/clusters.toml` (the ZCL specification as
  data), and a table-driven codec for every cluster command.
- `status`: status codes.

Nothing here is written per cluster: adding a cluster or an attribute is
an edit to the data file.

Part of [zigbee-bridge](https://github.com/pulzar-net/zigbee-bridge), maintained by
[Pulzar](https://pulzr.net). See the [user guide](https://pulzar-net.github.io/zigbee-bridge/), the
[library guide](https://pulzar-net.github.io/zigbee-bridge/library/index.html) and the
[API documentation](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_zcl/index.html).

License: MIT.
