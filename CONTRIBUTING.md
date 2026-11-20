# Contributing to zigbee-bridge

Thanks for helping. Every kind of contribution is welcome: a report that a
coordinator or device works, a bug report with a log, a fix to the docs, a
new cluster, a new coordinator family.

Please follow the [code of conduct](CODE_OF_CONDUCT.md). Security problems go
through [private reporting](SECURITY.md), never a public issue.

## Before you start

- **Hardware reports and device requests:** use the
  [device or hardware form](https://github.com/pulzar-net/zigbee-bridge/issues/new?template=device.yml).
- **Bugs:** use the [bug form](https://github.com/pulzar-net/zigbee-bridge/issues/new?template=bug.yml),
  with the version, your configuration and a `RUST_LOG=debug` log.
- **Larger changes** (a new coordinator family, a change to the MQTT API or
  the configuration): open an issue first, so we can agree on the approach
  before you write the code.

## Build

You need Rust 1.89 or newer ([rustup.rs](https://rustup.rs)). Nothing else:
the build has no system dependencies, and the tests start their own MQTT
broker.

```sh
git clone https://github.com/pulzar-net/zigbee-bridge
cd zigbee-bridge
cargo build --workspace
```

## Test

Before you send a change, run what CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

The tests drive the real host against the coordinator simulator, so a
change to formation, the trust center, interviews or the bridge is tested
end to end without hardware. See [Testing](https://pulzar-net.github.io/zigbee-bridge/testing.html)
for what each suite covers.

If you change a configuration type, regenerate the configuration reference
and commit the result:

```sh
UPDATE_DOCS=1 cargo test -p zigbee-bridge --features schema --test config_reference
```

## Run the simulator

To try a change by hand, run the simulated coordinator and point the bridge
at it:

```sh
cargo run -p zigbee-sim --bin zigbee-ncp-sim -- --listen 127.0.0.1:9999 --devices th,plug,door --report-every 10
cargo run -p zigbee-bridge -- --config my-test.toml   # [serial] port = "tcp://127.0.0.1:9999"
```

[Getting started](https://pulzar-net.github.io/zigbee-bridge/getting-started.html)
walks through it with an MQTT broker.

## Fuzz

Every parser of untrusted bytes has a fuzz target in `fuzz/`. If you change a
parser, fuzz it for a few minutes (needs a nightly toolchain and
`cargo install cargo-fuzz`):

```sh
cargo +nightly fuzz list
cargo +nightly fuzz run zcl_frames -- -max_total_time=300
```

A new parser needs a new fuzz target.

## Code rules

- No `unsafe` code; the workspace lints forbid it.
- No panics on input from a radio, a serial port or MQTT: parse with
  `zigbee_types::wire::Reader`, return errors, and never `unwrap` a value
  that came from outside.
- Allocate in proportion to the input, never to a length the input claims.
- Every public item has a doc comment (`missing_docs` is on).
- New behavior comes with a test: a unit test against the specification, or
  a simulator test.
- Device-specific behavior belongs in data (`crates/zigbee-zcl/data/clusters.toml`)
  where it can, not in code.
- Write from specifications and public vendor documentation. Do not copy
  code from other Zigbee projects, whatever their license.

## Commits and pull requests

- One logical change per commit. The subject line says what the change does,
  in plain words, under 72 characters, for example
  `Channel change on every coordinator family`; the body says why.
- Add a line to `CHANGELOG.md` under `[Unreleased]` for anything a user or a
  library user would notice.
- Update the README and `docs/` when you change behavior, a configuration
  key or an MQTT topic.
- Sign off every commit (see below).

CI must pass before a pull request is merged.

## Developer Certificate of Origin

Contributions are accepted under the [Developer Certificate of Origin
1.1](https://developercertificate.org): by signing off a commit, you certify
that you wrote it, or otherwise have the right to submit it under the
project's MIT license. Sign off with `git commit -s`, which adds a line like:

```text
Signed-off-by: Your Name <you@example.com>
```

using your real name. Commits without a sign-off cannot be merged.

## License

By contributing, you agree that your contributions are licensed under the
[MIT license](LICENSE).
