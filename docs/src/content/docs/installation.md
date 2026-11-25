---
title: "Installation"
description: "zigbee-bridge is one static Linux binary."
---

zigbee-bridge is one static Linux binary. Pick one of the ways below, then
[configure](/zigbee-bridge/configuration.html) it.

## Release binaries

Each [release](https://github.com/pulzar-net/zigbee-bridge/releases) has
statically linked binaries with no runtime dependencies, and a `SHA256SUMS`
file:

| File | For |
|---|---|
| `zigbee-bridge-vX.Y.Z-x86_64-unknown-linux-musl` | PCs and servers |
| `zigbee-bridge-vX.Y.Z-aarch64-unknown-linux-musl` | Raspberry Pi 3, 4, 5 and other 64-bit ARM boards with a 64-bit OS |
| `zigbee-bridge-vX.Y.Z-mipsel-unknown-linux-musl` | MT7621 and MT7628 routers (little-endian MIPS) |

Download, verify and install (this is the README's quick start; CI runs it):

```sh
VERSION=0.4.0
ARCH=$(uname -m)   # x86_64 or aarch64
BASE=https://github.com/pulzar-net/zigbee-bridge/releases/download/v$VERSION
curl -fsSLO "$BASE/zigbee-bridge-v$VERSION-$ARCH-unknown-linux-musl"
curl -fsSLO "$BASE/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS
sudo install -m 0755 "zigbee-bridge-v$VERSION-$ARCH-unknown-linux-musl" /usr/local/bin/zigbee-bridge
zigbee-bridge --version
```

There are no builds for macOS or Windows: build from source there. Big-endian
MIPS routers (such as Atheros ath79) are not supported.

## From crates.io

With Rust 1.89 or newer:

```sh
cargo install --locked zigbee-bridge
```

## From source

From a clone, `cargo build --release -p zigbee-bridge`; the binary is
`target/release/zigbee-bridge`. On Linux, opening serial ports needs nothing
beyond the C library; no system packages are required.

## As a systemd service

The repository has a hardened unit in
[`contrib/systemd/zigbee-bridge.service`](https://github.com/pulzar-net/zigbee-bridge/blob/main/contrib/systemd/zigbee-bridge.service).
It runs the bridge as an unprivileged dynamic user in the `dialout` group,
keeps its data in `/var/lib/zigbee-bridge`, and restarts it on failure.

```sh
sudo mkdir -p /etc/zigbee-bridge
sudo cp config.example.toml /etc/zigbee-bridge/config.toml   # then edit it
sudo install -m 0644 contrib/systemd/zigbee-bridge.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now zigbee-bridge
journalctl -u zigbee-bridge -f
```

The unit expects the binary at `/usr/local/bin/zigbee-bridge`. If your
distribution uses another group for serial devices (`uucp` on Arch Linux),
change `SupplementaryGroups`. If the broker needs a password, the unit has a
commented `LoadCredential` line that hands it to the bridge without making
the file readable to the bridge's user.

## On OpenWrt

> Needs real hardware: this procedure has not been tested on a router yet.

The `mipsel` binary runs on MT7621 and MT7628 routers with OpenWrt.

1. Install the USB serial driver for your coordinator's USB chip, for
   example `apk add kmod-usb-serial-cp210x` (OpenWrt 25.x) or
   `opkg install kmod-usb-serial-cp210x` (24.x and older) for a CP210x, or
   `kmod-usb-acm` for a CDC ACM device.
2. Copy the binary to `/usr/bin/zigbee-bridge` and make it executable.
3. Write `/etc/zigbee-bridge/config.toml`. **Set `[data] dir` to a directory
   on flash**, such as `/etc/zigbee-bridge/data`: `/var` is in RAM on
   OpenWrt, and the network key would be lost at reboot.
4. Install the procd script from
   [`contrib/openwrt/zigbee-bridge.init`](https://github.com/pulzar-net/zigbee-bridge/blob/main/contrib/openwrt/zigbee-bridge.init):

```sh
cp zigbee-bridge.init /etc/init.d/zigbee-bridge
chmod +x /etc/init.d/zigbee-bridge
/etc/init.d/zigbee-bridge enable
/etc/init.d/zigbee-bridge start
logread -f -e zigbee-bridge
```

The bridge writes to the data directory only when a device record changes
and once an hour for the backup, so flash wear is small. Add
`/etc/zigbee-bridge` to `/etc/sysupgrade.conf` so a firmware upgrade keeps
it.

## Command line

```text
zigbee-bridge [--config FILE]                 run the bridge
zigbee-bridge probe PORT [--baud N]           say what coordinator is on PORT
zigbee-bridge restore BACKUP [--config FILE]  restore BACKUP onto the coordinator at the next start
zigbee-bridge --version
```

`FILE` defaults to `/etc/zigbee-bridge/config.toml`. `PORT` is a serial
device or `tcp://host:port`. Exit status: 0 on a clean stop, 1 on an error,
2 for bad arguments or configuration, and 75 when the bridge must be started
again to finish a [restore](/zigbee-bridge/backup.html).

The log goes to standard error. `RUST_LOG` sets its level, for example
`RUST_LOG=debug`, or `RUST_LOG=info,zigbee_ash=trace` to see every serial
frame.
