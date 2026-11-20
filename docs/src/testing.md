# Testing

The whole stack is developed and tested without hardware. CI runs every
test on every commit; most run in seconds on a laptop.

## Running the tests

```sh
cargo test --workspace                                   # everything below except fuzzing
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Some tests need an MQTT broker; they start their own in-process
([rumqttd](https://crates.io/crates/rumqttd)), so nothing needs to be
installed.

## Unit tests

Every crate tests its codecs against the specification: ASH framing against
the byte-level examples in Silicon Labs UG101, EZSP and ZNP commands in both
directions, ZCL values and frames, ZDO descriptors, and install code key derivation against the published test
vector.

## The simulator

[zigbee-sim](library/zigbee-sim.md) plays a coordinator, with simulated
devices behind it. Tests connect the real host to it over an in-memory pipe,
so they exercise the same code paths as hardware: the serial protocol,
formation, joins, the trust center key exchange, interviews, reporting,
commands, backups, restores and topology.

- `crates/zigbee-host/tests/sim.rs` runs the host against every coordinator
  family and protocol version the simulator plays (EZSP 13 and 16, Z-Stack
  3.x.0 and 3.0.x).
- `crates/zigbee-bridge/tests/e2e.rs` runs the bridge end to end: simulator,
  bridge and an MQTT broker, checking topics, requests and Home Assistant
  discovery.
- `ci/quickstart.sh` runs the README's quick start as written, with the
  release binary and with the current build, against the simulator.

To try things by hand, run `zigbee-ncp-sim` and point a bridge at it; see
[getting started](getting-started.md).

## Recorded sessions

None yet. Once real coordinators are tested, serial captures from them will
be added as replay tests, so behavior seen on hardware stays covered.

## Fuzzing

Every parser that reads untrusted bytes has a
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) target in `fuzz/`:
ASH frames and the ASH connection state machine, EZSP, ZNP and deCONZ
frames, ZCL and ZDO frames, and OTA image headers. CI fuzzes each one for 30
seconds on every commit. To fuzz longer locally (needs a nightly toolchain):

```sh
cargo install cargo-fuzz
cargo +nightly fuzz list
cargo +nightly fuzz run zcl_frames -- -max_total_time=600
```

A crash leaves its input in `fuzz/artifacts/`; add it to the crate's unit
tests when you fix it.

## Budgets

- **Memory:** `crates/zigbee-host/tests/budget.rs` runs 200 simulated
  devices and fails above 16 MB resident. It is ignored by default; run it
  with `cargo test --release -p zigbee-host --test budget -- --ignored`.
- **Binary size:** CI builds static `aarch64` and `mipsel` binaries and fails
  if one is over 4 MB.
- **Other architectures:** CI runs the whole test suite on `aarch64` and
  `mipsel` under qemu.

## Documentation

The `docs` workflow checks the claims this site makes: the
[configuration reference](config-reference.md) is regenerated from the code
and compared, every doctest runs (the README's library example among them),
the README's quick start runs, the workspace builds on the minimum supported
Rust version, and every relative link resolves.

## Real hardware

> Not done yet. Testing on real coordinators and devices is the next
> milestone.

Until then, anything that only real hardware can confirm (radio behavior,
specific firmware versions, specific devices) is marked as such in these
docs.
