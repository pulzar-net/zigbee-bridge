# zigbee-sim

A Zigbee network co-processor simulator, for testing Zigbee hosts without
hardware.

`Simulator` plays one coordinator family over any byte stream:
Silicon Labs EmberZNet (ASH and EZSP, protocol 13 as EmberZNet 7.4 does,
or 14 and later), Texas Instruments Z-Stack 3 (ZNP) or dresden
elektronik deCONZ (a ConBee II). It keeps a
network as an NCP keeps its non-volatile memory (keys, PAN ids, frame
counter, link keys, a written IEEE address), and runs simulated
`SimDevice`s that join when the network opens, answer ZDO and ZCL
requests, keep bindings and reporting configuration, and report
attributes the way they were configured to.

The `zigbee-ncp-sim` binary serves it over TCP, so a bridge can be pointed
at `tcp://127.0.0.1:<port>`.

Part of [zigbee-bridge](https://github.com/pulzar-net/zigbee-bridge), maintained by
[Pulzar](https://pulzr.net). See the [user guide](https://oss.pulzr.net/zigbee-bridge/), the
[library guide](https://oss.pulzr.net/zigbee-bridge/library/index.html) and the
[API documentation](https://oss.pulzr.net/zigbee-bridge/api/zigbee_sim/index.html).

License: MIT.
