# Contributing to zigbee-bridge

Thanks for helping. Every kind of contribution is welcome: a report that a
coordinator or device works, a bug report with a log, a fix to the docs, a
new cluster, a new coordinator family.

Please follow the [code of conduct](CODE_OF_CONDUCT.md). Security problems go
through [private reporting](SECURITY.md), never a public issue.

## Before you start

- **Questions** (how do I, is this expected): ask in
  [Discussions](https://github.com/pulzar-net/zigbee-bridge/discussions).
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
end to end without hardware. See [Testing](https://oss.pulzr.net/zigbee-bridge/testing.html)
for what each suite covers.

If you change a configuration type, regenerate the configuration reference
and commit the result:

```sh
UPDATE_DOCS=1 cargo test -p zigbee-bridge --features schema --test config_reference
```

If you change an MQTT payload, regenerate the MQTT reference the same way
(see [The MQTT reference](#the-mqtt-reference) below):

```sh
UPDATE_DOCS=1 cargo test -p zigbee-bridge --features unstable-asyncapi-docs --test asyncapi
```

## The MQTT reference

The [MQTT reference](https://oss.pulzr.net/zigbee-bridge/mqtt-reference.html)
is an AsyncAPI 3.0 description generated from the bridge's own payload
types, so it cannot drift from what the bridge publishes. The other Pulzr
bridges follow the same pattern:

- **One module holds every payload** (`crates/zigbee-bridge/src/api.rs`): each
  topic's message, each event, each request and its response. The bridge
  serializes these types and never builds JSON by hand, so they are the
  contract.
- **The derives sit behind a non-default feature**, `unstable-asyncapi-docs`.
  It turns on two optional dependencies, `schemars` and `asyncapi-rust`.
  Default builds, release binaries and cross builds never enable it, and CI
  fails if a default build pulls in either crate. The feature is docs
  tooling, not API, and may change in any release.

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[cfg_attr(feature = "unstable-asyncapi-docs", derive(JsonSchema, ToAsyncApiMessage))]
  #[cfg_attr(feature = "unstable-asyncapi-docs", asyncapi(summary = "A device joined"))]
  pub struct DeviceJoined { /* ... */ }
  ```

  Every `#[asyncapi(...)]` or `#[schemars(...)]` attribute goes inside
  `cfg_attr` too.
- **Doc comments are the descriptions.** Each field's `///` comment becomes
  its description in the reference, so write them for the person reading it:
  units, ranges, when a value is `null`.
- **Name tagged-enum variants one by one** with `#[serde(rename = "...")]`,
  not `rename_all`. asyncapi-rust 0.5 splits a tagged enum into one message
  per variant only for explicit renames (mlilback/asyncapi-rust#25). A
  payload that is a bare string goes in a `#[serde(transparent)]` newtype,
  so it becomes one message rather than one per variant.
- **The generator is a test** with `required-features`
  (`crates/zigbee-bridge/tests/asyncapi.rs`). It holds the
  `#[derive(AsyncApi)]` struct with servers, channels, operations and MQTT
  bindings (QoS, retain, last will), and writes `docs/public/asyncapi.json`
  when `UPDATE_DOCS` is set. Otherwise it fails if the committed file
  differs.
- **CI checks both directions.** The docs workflow runs that test, then
  `@asyncapi/cli validate --fail-severity=warn`, with the CLI's analytics
  turned off first.
- **The docs page** is one `.mdx` file using `AsyncApiReference` from
  `@pulzr/docs-theme`, with `tableOfContents: false`. The hand-written MQTT
  page keeps the explanations and links to the reference, with no
  hand-maintained field tables.
- **The dependency is upstream asyncapi-rust**, as
  `asyncapi-rust = { version = "0.5", optional = true }`, and that line never
  changes. When upstream lacks something the reference needs, the fix goes
  to the `pulzar-net/asyncapi-rust` fork, which is never published, and the
  workspace pins the fork with `[patch.crates-io]` at a fixed revision for
  `asyncapi-rust`, `asyncapi-rust-codegen` and `asyncapi-rust-models`. The
  same fix is offered upstream as a pull request, and the patch goes once a
  release has it. `cargo publish` ignores patches and the published crate
  only uses the feature's derives, so publishing is unaffected; CI's
  `cargo publish --dry-run` checks it.

## Run the simulator

To try a change by hand, run the simulated coordinator and point the bridge
at it:

```sh
cargo run -p zigbee-sim --bin zigbee-ncp-sim -- --listen 127.0.0.1:9999 --devices th,plug,door --report-every 10
cargo run -p zigbee-bridge -- --config my-test.toml   # [serial] port = "tcp://127.0.0.1:9999"
```

[Getting started](https://oss.pulzr.net/zigbee-bridge/getting-started.html)
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
  `zigbee_primitives::wire::Reader`, return errors, and never `unwrap` a value
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
- Say in the pull request what a user or a library user would notice: it
  goes into the
  [release notes](https://github.com/pulzar-net/zigbee-bridge/releases).
  There is no changelog file; a release's notes are its annotated tag's
  message (`git tag -a vX.Y.Z --cleanup=whitespace -F notes.md`), which the
  release workflow publishes.
- Update the README and `docs/` when you change behavior, a configuration
  key or an MQTT topic.
- Sign off every commit (see below).

CI must pass before a pull request is merged: the tests, fuzzing and the
cross builds. Pull requests are squash-merged, so the pull
request's title and description become the commit on `main`: write them
as you would a commit message.

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
