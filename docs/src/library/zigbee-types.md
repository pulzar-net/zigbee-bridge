# zigbee-types

The types every other crate shares. [API docs](https://pulzar-net.github.io/zigbee-bridge/api/zigbee_types/index.html).

| Type | What it is |
|---|---|
| `Ieee` | a 64-bit IEEE address; parses `00124b0001020304`, `0x00124B0001020304` and `00:12:4b:00:01:02:03:04`; `to_prefixed_hex()` gives `0x00124b0001020304` |
| `ExtPanId` | an extended PAN id |
| `Key` | a 128-bit key whose `Debug` never prints the key |
| `NodeId` | a 16-bit network address |
| `wire::Reader` | a bounds-checked byte reader |

`wire::Reader` is how every parser in the workspace reads bytes: it returns
an error instead of panicking at the end of input, so a malformed frame from
a radio can never crash the process. Use it for your own parsers too.

Feature `serde` (default) serializes addresses and keys as hex strings.
