# The library crates

zigbee-bridge is built as a stack of library crates. The bridge binary is
the thinnest layer on top; everything else can be used without it, to build
a different bridge, a gateway, a test tool or a protocol analyzer.

| Crate | Layer | Use it to |
|---|---|---|
| [zigbee-host](zigbee-host.md) | coordinator application | run a Zigbee network from Rust |
| [zigbee-zcl](zigbee-zcl.md) | Zigbee Cluster Library | encode and decode ZCL frames and values |
| [zigbee-zdo](zigbee-zdo.md) | Zigbee Device Objects | encode and decode ZDO requests and responses |
| [zigbee-ezsp and zigbee-ash](zigbee-ezsp.md) | Silicon Labs serial protocols | talk to an EmberZNet NCP directly |
| [zigbee-znp](zigbee-znp.md) | Texas Instruments serial protocol | talk to a Z-Stack NCP directly |
| [zigbee-deconz](zigbee-deconz.md) | dresden elektronik serial protocol | talk to a ConBee or RaspBee directly |
| [zigbee-types](zigbee-types.md) | shared types | addresses, keys, a safe wire reader |
| [zigbee-sim](zigbee-sim.md) | test support | test Zigbee software without hardware |
| [zigbee-bridge](zigbee-bridge.md) | MQTT bridge | run or embed the bridge itself |

The [API documentation](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_host/index.html) covers every public item.
This guide explains what each crate is for and how the pieces fit.

## Depending on the crates

The crates are not published on crates.io yet. Depend on a release tag:

```toml
[dependencies]
zigbee-host = { git = "https://github.com/pulzar-net/zigbee-bridge", tag = "v0.2.0" }
```

Every crate shares the workspace version and is released together. While
the project is at 0.x, minor versions may break the API; the
[changelog](../changelog.md) says what changed.

## Conventions shared by every crate

- **No `unsafe`.** The workspace lints forbid `unsafe` code in every crate.
- **No panics on input.** Everything that parses bytes from a radio or a
  serial port goes through the bounds-checked reader in zigbee-types, and
  is fuzzed. A malformed frame is an error value, never a panic.
- **Bounded allocation.** Parsers allocate in proportion to their input,
  never to a length field the input claims.
- **Sans-IO where it matters.** Protocol state machines (the ASH link, frame
  decoders) take bytes and time as input and return actions, so they can be
  tested without I/O and driven by any runtime. The async clients are thin
  [tokio](https://tokio.rs) drivers around them.
- **Encoding in both directions.** Frame types encode and decode both the
  host's and the NCP's or device's side, so the simulator shares the
  definitions the host uses.
