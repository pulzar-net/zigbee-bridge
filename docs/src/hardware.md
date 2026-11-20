# Hardware and firmware

zigbee-bridge drives a Zigbee coordinator: a radio module running network
co-processor (NCP) firmware, attached over USB serial, a UART, or TCP. The
firmware decides whether it works, more than the hardware does.

> **No release has been tested on real hardware yet.** Everything below
> "expected" is what the stack implements and the simulator covers. Reports
> that something works, or does not, are very welcome: open a
> [hardware report](https://github.com/pulzar-net/zigbee-bridge/issues/new?template=device.yml).

## Coordinators

| Coordinator | Chip | Connection | Firmware | State |
|---|---|---|---|---|
| Home Assistant Connect ZBT-1 (SkyConnect) | EFR32MG21 | USB | EmberZNet 7.4 or newer, Zigbee NCP | expected |
| Sonoff ZBDongle-E | EFR32MG21 | USB | EmberZNet 7.4 or newer (ships with 6.10: update it) | expected |
| SMLIGHT SLZB-07 | EFR32MG21 | USB | EmberZNet 7.4 or newer | expected |
| SMLIGHT SLZB-06M | EFR32MG21 | Ethernet, `tcp://` | EmberZNet 7.4 or newer | expected |
| Other EFR32MG21, MG24 coordinators | EFR32MG2x | USB, UART, TCP | EmberZNet 7.4 to 8.x | expected |
| Sonoff ZBDongle-P, SMLIGHT SLZB-06 | CC2652P | USB, TCP | Z-Stack 3.x.0 | next release (on `main`) |
| Other CC2652, CC1352 coordinators | CC26x2, CC13x2 | USB, UART, TCP | Z-Stack 3.x.0 | next release (on `main`) |
| CC2531 | CC2531 | USB | Z-Stack 3.0.x | next release (on `main`), not recommended |
| ConBee II, ConBee III, RaspBee II | deCONZ | USB, GPIO UART | deCONZ firmware | next release (on `main`) |
| EFR32 with EmberZNet 7.3 or older | EFR32 | | EZSP 12 or older | unsupported: refused, update the firmware |
| CC2530, CC2531 with Z-Stack Home 1.2 | CC253x | | Z-Stack 1.2 | unsupported |
| XBee, ZiGate, Telink, NXP | | | | unsupported, not planned |

The firmware must be the **Zigbee NCP** build (also called "coordinator" or
"ncp-uart"). Router, RCP (multiprotocol) and Thread firmware for the same
stick will not work.

### Silicon Labs EmberZNet

The stack speaks EZSP protocol versions 13 to 19, which EmberZNet 7.4.0 to
8.x firmware uses. Older firmware (EZSP 12 and older) is detected and refused
with a message to update it; `zigbee-bridge probe` still identifies it.

Firmware builds are named for their flow control: `ncp-uart-hw` builds use
RTS/CTS (`flow_control = "hardware"`), `ncp-uart-sw` builds use XON/XOFF
(`flow_control = "software"`). Most run at 115200 baud; some builds run at
460800 (`baud = 460800`).

### dresden elektronik deCONZ

On `main`, not in 0.1.0. ConBee II, ConBee III, RaspBee and RaspBee II with
dresden elektronik's deCONZ firmware, over its serial protocol. Use the
firmware version dresden elektronik currently recommends for your stick.

### Texas Instruments Z-Stack

On `main`, not in 0.1.0. Z-Stack 3.x.0 (CC2652, CC1352, CC26x2) and Z-Stack
3.0.x (CC2530, CC2531) coordinator firmware, over ZNP. A CC2531 works but is
slow and has very little memory; it is fine for a test, not for a house.

## Check a coordinator

`probe` sends one reset and a few read-only queries, prints what it found,
and closes the port. It never writes anything and finishes within a few
seconds:

```sh
zigbee-bridge probe /dev/serial/by-id/usb-Nabu_Casa_SkyConnect_v1.0_...
```

```json
{"kind":"ezsp","firmware":"7.4.4.0","protocol":13,"supported":true,"ieee_address":"0x...","manufacturer":"...","board":"..."}
```

`supported: false` means the firmware is too old. No output and exit status
1 means nothing answered: check the port, `--baud`, and that the firmware is
an NCP build. Stop anything else using the port (another bridge, ModemManager)
first.

## Flashing firmware

> Needs real hardware. Flashing replaces the firmware on the stick and can
> brick it if interrupted. Follow the vendor's instructions where they
> differ from this summary.

### EFR32 coordinators

Use Nabu Casa's
[universal-silabs-flasher](https://github.com/NabuCasa/universal-silabs-flasher)
(Python), which detects what is running and flashes `.gbl` images:

```sh
pip install universal-silabs-flasher
universal-silabs-flasher --device /dev/serial/by-id/usb-... probe
universal-silabs-flasher --device /dev/serial/by-id/usb-... flash --firmware <firmware>.gbl
```

Firmware images:

- Home Assistant Connect ZBT-1: Nabu Casa's
  [silabs-firmware-builder](https://github.com/NabuCasa/silabs-firmware-builder/releases)
  releases (the Zigbee NCP images), or Home Assistant's own firmware update
  for the stick.
- Sonoff ZBDongle-E and other EFR32MG21 sticks: the
  [darkxst/silabs-firmware-builder](https://github.com/darkxst/silabs-firmware-builder/releases)
  releases, or the vendor's own.
- SMLIGHT devices: the firmware page of the device's web interface.

### CC2652 coordinators

Use [cc2538-bsl](https://github.com/JelmerT/cc2538-bsl) with the coordinator
images from [Koenkk/Z-Stack-firmware](https://github.com/Koenkk/Z-Stack-firmware):

```sh
python3 cc2538-bsl.py -p /dev/ttyUSB0 -evw <firmware>.hex
```

Pick the image for your chip and board. Many sticks enter the bootloader by
holding a button while plugging in; see the stick's documentation.

### ConBee and RaspBee

dresden elektronik's
[GCFFlasher](https://github.com/dresden-elektronik/gcfflasher) and the
firmware from their [download site](https://deconz.dresden-elektronik.de/deconz-firmware/).

## Devices

The bridge has no per-device database. Any device that implements the
standard Zigbee Cluster Library clusters reports through them; the
[MQTT API](mqtt.md#device-state) lists which clusters become which fields,
and [Home Assistant](home-assistant.md#what-is-created) which entities they
become. Every attribute report is also published raw on
`zigbee/<device>/report`, so nothing a device sends is lost.

What does not work yet: devices that put their readings in
manufacturer-specific clusters or non-standard attributes (common with
Tuya and Xiaomi/Aqara devices), and devices that need a manufacturer-specific
setup step. Support for those is on the [roadmap](https://github.com/pulzar-net/zigbee-bridge#roadmap).

## Radio placement

Zigbee shares the 2.4 GHz band with Wi-Fi, and USB 3 ports radiate noise in
the same band. Put the coordinator on a USB 2 extension cable, away from the
computer and from USB 3 devices. Choose a channel away from your Wi-Fi: the
[`energy_scan`](mqtt.md#energy_scan) request measures every channel, and
`network.channel` sets the channel of a new network. Channels 15, 20 and 25
sit between the common Wi-Fi channels 1, 6 and 11.
