# zigbee-bridge

A standalone Zigbee to MQTT bridge on the zigbee-host stack.

`run` opens the coordinator, starts (or resumes, or restores) the
network, connects to the MQTT broker and bridges until told to stop.
The topic scheme is in `bridge`; Home Assistant discovery is in
`exposes`.

Part of [zigbee-bridge](https://github.com/pulzar-net/zigbee-bridge), maintained by
[Pulzar](https://pulzr.net). See the [user guide](https://oss.pulzr.net/zigbee-bridge/), the
[library guide](https://oss.pulzr.net/zigbee-bridge/library/index.html) and the
[API documentation](https://oss.pulzr.net/zigbee-bridge/api/zigbee_bridge/index.html).

License: MIT.
