# zigbee-types

Address, key and wire-format types shared by the zigbee-bridge crates.

Everything a Zigbee frame parser needs in common lives here: the 64-bit
IEEE address (`Ieee`), the extended PAN id (`ExtPanId`), 128-bit keys
(`Key`, whose `Debug` never prints the key), and a bounds-checked byte
reader (`wire::Reader`) that every parser in the workspace uses instead
of slicing, so that a malformed frame is an error and never a panic.

Part of [zigbee-bridge](https://github.com/pulzar-net/zigbee-bridge), maintained by
[Pulzar](https://pulzr.net). See the [user guide](https://pulzar-net.github.io/zigbee-bridge/), the
[library guide](https://pulzar-net.github.io/zigbee-bridge/library/index.html) and the
[API documentation](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_types/index.html).

License: MIT.
