# Backup and restore

A Zigbee network lives in the coordinator's non-volatile memory: the network
key, its frame counter, the PAN ids, and the trust center's link keys. If the
coordinator dies and nothing else holds them, every device must be reset
and paired again. zigbee-bridge keeps a backup so that never has to happen.

## The backup

The bridge writes `backup.json` to the data directory:

- on start, unless it resumed the network unchanged;
- after every join and leave;
- every hour;
- on the [`backup`](mqtt.md#backup) request.

It uses the open coordinator backup format
(`"format": "zigpy/open-coordinator-backup"`, version 1), the JSON format
zigpy (ZHA) and Zigbee2MQTT also write:

```json
{
  "metadata": {
    "format": "zigpy/open-coordinator-backup",
    "version": 1,
    "source": "zigbee-bridge@0.1.0",
    "internal": { "created_unix": 1795136470, "ezspVersion": 13, "firmware": "7.4.4.0" }
  },
  "stack_specific": { "ezsp": { "hashed_tclk": "<32 hex digits>" } },
  "coordinator_ieee": "000d6ffffe000001",
  "pan_id": "74fe",
  "extended_pan_id": "28298b99d1d288ef",
  "nwk_update_id": 0,
  "security_level": 5,
  "channel": 15,
  "channel_mask": [11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26],
  "network_key": { "key": "<32 hex digits>", "sequence_number": 0, "frame_counter": 60 },
  "devices": [
    { "nwk_address": "2b3c", "ieee_address": "00124b0000000001", "is_child": true }
  ]
}
```

It holds the network key and the trust center's root key, from which every
device's unique link key is derived: anyone with it can
join and decrypt your network. The bridge writes it with mode 0600 and never
publishes it on MQTT. Copy it off the machine (with the rest of the data
directory) as part of your normal backups, and store it as a secret.

Moving a network between zigbee-bridge and ZHA or Zigbee2MQTT through this
file should work, since they share the format, but has not been tested yet.

## Restoring onto a new coordinator

> Needs real hardware: restore is tested against the simulator only.

To replace a dead or outgrown coordinator with another of the same family,
without pairing devices again:

1. Stop the bridge and plug in the new coordinator. Point `serial.port` at it.
2. Stage the backup:

   ```sh
   zigbee-bridge restore /var/lib/zigbee-bridge/backup.json
   ```

   This checks the file and copies it to `restore-pending.json` in the data
   directory. Nothing is written to the coordinator yet.
3. Start the bridge. It writes the old coordinator's IEEE address to the new
   one, then exits with status 75 (`EX_TEMPFAIL`). The systemd unit and the
   OpenWrt script restart it; if you run it by hand, start it again.
4. On the next start it restores the network: PAN ids, channel, network key,
   link keys, and the network frame counter advanced past anything the old
   coordinator could have sent (by 100,000), so devices accept the new coordinator's
   frames.

Devices then talk to the new coordinator as if nothing happened. Sleepy end
devices may take until their next wake-up to show up again.

Limits:

- **On EmberZNet, the IEEE address can be written once.** It is stored in a
  one-time token. A coordinator whose address was already written (by an
  earlier restore or by another tool) is refused, with the reason.
- **Coordinator family.** In 0.1.0 a backup restores onto EmberZNet. On
  `main`, it restores onto any supported family: the IEEE address, PAN ids,
  channel, network key and frame counter always; the trust center's root key
  (EmberZNet) or link key seed (Z-Stack) when restoring onto the same family,
  and the backup's per-device link keys otherwise. Backups written by other
  hosts in the same format can be read too.
- **Keep the old coordinator unplugged.** Two coordinators with one network
  confuse every device.

## Starting over

To form a new network on the same coordinator, stop the bridge, move
`network.json`, `devices.json` and `backup.json` out of the data directory,
set `network.replace_other = true`, and start it. It refuses to replace a
foreign network unless told to, so a coordinator with a network on it is
never wiped by accident. Set `replace_other` back to `false` afterwards.
Every device must then be reset and paired again.
