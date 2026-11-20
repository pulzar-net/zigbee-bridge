---
title: "Troubleshooting and FAQ"
description: "Start with the log."
---

Start with the log. Run the bridge with `RUST_LOG=debug` to see every
request and report; see [logging](/zigbee-bridge/configuration.html#logging) for more.

## The bridge does not start

**`no EZSP or ZNP coordinator answered on PORT`, or `probe` prints nothing.**
The port is right but nothing speaks a supported protocol on it.

- Is the port in use? Stop other Zigbee software (Zigbee2MQTT, ZHA, deCONZ)
  and ModemManager, which probes new serial devices:
  `sudo systemctl stop ModemManager`.
- Is the baud rate right? Try `zigbee-bridge probe PORT --baud 460800`.
- Is it NCP firmware? Router, RCP (multiprotocol) and Thread builds do not
  answer. See [flashing firmware](/zigbee-bridge/hardware.html#flashing-firmware).
- Does the flow control match the firmware build? See
  [Silicon Labs EmberZNet](/zigbee-bridge/hardware.html#silicon-labs-emberznet).

**`Permission denied` opening the port.** Your user (or the service's) needs
to be in the group that owns the device, usually `dialout`
(`ls -l /dev/ttyUSB0` shows it). The [systemd unit](/zigbee-bridge/installation.html#as-a-systemd-service)
adds it.

**`firmware ... is too old; update it`.** EmberZNet before 7.4 speaks EZSP 12
or older. [Update the firmware](/zigbee-bridge/hardware.html#flashing-firmware).

**The coordinator runs a different network.** The coordinator already has a
network that is not the one in `network.json` (it was used with other
software, or `network.json` was deleted). The bridge refuses to wipe it. To
keep that network, restore its backup from the other software. To replace
it, see [starting over](/zigbee-bridge/backup.html#starting-over).

**`network.channel ... is not 11 to 26`, `unknown field`, `missing field`.**
The configuration file has an error; the message names the key. See the
[configuration reference](/zigbee-bridge/config-reference.html).

**The bridge exits with status 75.** It wrote the IEEE address for a
[restore](/zigbee-bridge/backup.html#restoring-onto-a-new-coordinator) and must be started
again.

## Devices

**A device does not join.**

- Was the network open? Send `permit_join` and watch for the `permit_join`
  event. The window lasts at most 254 seconds.
- Reset the device to factory defaults; most refuse to join while they still
  belong to another network.
- With `join.install_code_only = true`, add its install code first.
- Bring the device close to the coordinator or a router while pairing.
- Check the log for the join. A `device_joined` event followed by a
  `device_leave` usually means the device failed the key exchange; with
  `join.allow_legacy = false`, devices older than Zigbee 3.0 are removed.

**The interview fails or never finishes.** Battery devices sleep, and miss
requests. Wake the device (press its button a few times) during the
interview. A failed interview resumes on its own the next time the device
talks; you can also send the `interview` request.

**A device joined but shows no readings.** Look at `zigbee/<device>/report`:

- Reports arrive there but not in the state: the device uses a cluster or
  attribute the bridge does not map to a field yet (often manufacturer
  specific). [Tell us about it](https://github.com/pulzar-net/zigbee-bridge/issues/new?template=device.yml),
  with the report.
- Nothing arrives: the device does not report on its own. The interview
  configures reporting for the standard clusters, but some devices ignore
  it.

**Readings are late or missing; devices drop off.** Usually radio trouble.
See [radio placement](/zigbee-bridge/hardware.html#radio-placement). Add mains-powered
routers (plugs, bulbs) between the coordinator and distant devices. The
[`topology`](/zigbee-bridge/mqtt.html#topology) request and each device's `linkquality` show
weak links.

## Home Assistant

**Nothing appears in Home Assistant.** Check that both use the same broker,
that the MQTT integration has discovery on, and that
`homeassistant.discovery_prefix` matches Home Assistant's (default
`homeassistant`). Watch `homeassistant/#` with `mosquitto_sub`: discovery is
published when a device finishes its interview.

**Entities show as unavailable.** Home Assistant follows
`zigbee/bridge/state`. If it says `offline`, the bridge stopped or lost the
broker.

**A removed device came back.** Remove it with the bridge's `remove`
request, not only in Home Assistant. See
[removing a device](/zigbee-bridge/home-assistant.html#removing-a-device).

## FAQ

**Can I use zigbee-bridge and Zigbee2MQTT (or ZHA) on one coordinator?** No.
One coordinator runs one network for one host at a time. Run them on
separate coordinators, each with its own network.

**Can I move my network from Zigbee2MQTT or ZHA without pairing again?**
Probably, with their coordinator backup: it is the same open format. This
has not been tested yet. See [backup and restore](/zigbee-bridge/backup.html).

**Is the topic scheme compatible with Zigbee2MQTT?** It is modeled on it
(`bridge/request/permit_join`, `bridge/devices`, `<device>/set`, readings
named the same way), so most tools need little change. It is not
identical: payloads differ in places, and there are no per-device
converters.

**Does it support OTA firmware updates?** Since 0.2.0:
from image files you put in the OTA directory, one device at a time, on
request. See [`ota_update`](/zigbee-bridge/mqtt.html#ota_check-and-ota_update). There is no
automatic download of images.

**Groups or scenes?** Not yet. See the
[roadmap](https://github.com/pulzar-net/zigbee-bridge#roadmap).

**Why is my device's battery missing?** Battery entities are only created
for battery-powered devices, and only once the device reports its battery.
Many devices report it every few hours.

**Is there a web interface?** No. The bridge is headless; everything goes
through MQTT. Home Assistant or any MQTT tool is the interface.

**Where do I ask a question?** In
[Discussions](https://github.com/pulzar-net/zigbee-bridge/discussions).
Bugs go in [issues](https://github.com/pulzar-net/zigbee-bridge/issues).
For security problems, see [SECURITY.md](https://github.com/pulzar-net/zigbee-bridge/blob/main/SECURITY.md).
