//! The simulated network every NCP front-end shares: the stored network,
//! devices, joins, leaves and frame delivery. A front-end (EZSP, ZNP,
//! deCONZ) turns host commands into calls here and [`MeshEvent`]s into its
//! own protocol's frames.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use zigbee_types::{ExtPanId, Ieee, Key, NodeId, PanId};

use crate::device::{DeviceState, Outgoing, SimDevice};

/// The network the NCP has stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimNetwork {
    /// Channel.
    pub channel: u8,
    /// PAN id.
    pub pan_id: PanId,
    /// Extended PAN id.
    pub extended_pan_id: ExtPanId,
    /// Network update id.
    pub nwk_update_id: u8,
    /// Channels the network may use.
    pub channel_mask: u32,
    /// Network key.
    pub network_key: Key,
    /// Key sequence number.
    pub key_sequence: u8,
    /// The trust center's key material: EmberZNet's hashed link key root,
    /// Z-Stack's link key seed, or zero.
    pub tc_root: Key,
    /// Outgoing network frame counter.
    pub frame_counter: u32,
}

/// Something that happened on the simulated network.
#[derive(Debug, Clone)]
pub(crate) enum MeshEvent {
    /// A device joined.
    Joined {
        nwk: NodeId,
        ieee: Ieee,
        /// An end device (sleepy or not) rather than a router.
        end_device: Option<bool>,
        /// It asks for a unique link key (not a legacy device).
        key_exchange: bool,
    },
    /// A device left.
    Left { nwk: NodeId, ieee: Ieee },
    /// A device sent the coordinator a frame.
    Incoming {
        nwk: NodeId,
        ieee: Ieee,
        lqi: u8,
        rssi: i8,
        aps_seq: u8,
        out: Outgoing,
    },
}

/// Work for later, run by the serving loop.
#[derive(Debug)]
pub(crate) enum Scheduled {
    Join(Ieee),
    /// The trust center removes a device.
    Remove(Ieee),
    Mesh(MeshEvent),
    Ezsp(zigbee_ezsp::Callback),
    Znp(zigbee_znp::Indication),
}

pub(crate) struct Joined {
    pub nwk: NodeId,
    pub state: DeviceState,
}

pub(crate) struct Mesh {
    pub join_delay: Duration,
    pub reply_delay: Duration,
    pub factory_ieee: Ieee,
    pub ieee_override: Option<Ieee>,
    pub devices: BTreeMap<Ieee, SimDevice>,
    pub joined: BTreeMap<Ieee, Joined>,
    pub network: Option<SimNetwork>,
    pub up: bool,
    pub permit_until: Option<Instant>,
    /// The trust center admits joins.
    pub allow_joins: bool,
    /// Only devices with an install code key may join.
    pub install_code_only: bool,
    /// The well-known key is accepted for joins.
    pub well_known_ok: bool,
    /// Install code keys by device.
    pub join_keys: HashMap<Ieee, Key>,
    next_nwk: NodeId,
    aps_seq: u8,
    pub sink: Option<mpsc::UnboundedSender<(Duration, Scheduled)>>,
}

impl Mesh {
    pub fn new(
        factory_ieee: Ieee,
        devices: &[SimDevice],
        join_delay: Duration,
        reply_delay: Duration,
    ) -> Mesh {
        Mesh {
            join_delay,
            reply_delay,
            factory_ieee,
            ieee_override: None,
            devices: devices.iter().map(|d| (d.ieee, d.clone())).collect(),
            joined: BTreeMap::new(),
            network: None,
            up: false,
            permit_until: None,
            allow_joins: false,
            install_code_only: false,
            well_known_ok: false,
            join_keys: HashMap::new(),
            next_nwk: 0x1A2B,
            aps_seq: 0,
            sink: None,
        }
    }

    pub fn ieee(&self) -> Ieee {
        self.ieee_override.unwrap_or(self.factory_ieee)
    }

    pub fn schedule(&self, delay: Duration, what: Scheduled) {
        if let Some(tx) = &self.sink {
            let _ = tx.send((delay, what));
        }
    }

    pub fn next_aps_seq(&mut self) -> u8 {
        self.aps_seq = self.aps_seq.wrapping_add(1);
        self.aps_seq
    }

    /// The stack restarted: the stored network stays, the rest is gone.
    pub fn reset(&mut self) {
        self.up = false;
        self.permit_until = None;
        self.allow_joins = false;
        self.install_code_only = false;
        self.well_known_ok = false;
        self.join_keys.clear();
    }

    pub fn nwk_of(&self, ieee: Ieee) -> Option<NodeId> {
        self.joined.get(&ieee).map(|j| j.nwk)
    }

    pub fn ieee_of(&self, nwk: NodeId) -> Option<Ieee> {
        self.joined
            .iter()
            .find(|(_, j)| j.nwk == nwk)
            .map(|(i, _)| *i)
    }

    pub fn deliver(&mut self, from: Ieee, out: Outgoing, delay: Duration) {
        let Some(nwk) = self.nwk_of(from) else {
            return;
        };
        let (lqi, rssi) = self
            .devices
            .get(&from)
            .map(|d| (d.lqi, d.rssi))
            .unwrap_or((200, -50));
        if let Some(n) = self.network.as_mut() {
            n.frame_counter = n.frame_counter.wrapping_add(1);
        }
        let aps_seq = self.next_aps_seq();
        self.schedule(
            delay,
            Scheduled::Mesh(MeshEvent::Incoming {
                nwk,
                ieee: from,
                lqi,
                rssi,
                aps_seq,
                out,
            }),
        );
    }

    pub fn permit_open(&self) -> bool {
        self.permit_until.is_some_and(|t| Instant::now() < t)
    }

    /// Open (seconds > 0) or close joining; devices in pairing mode join.
    pub fn permit(&mut self, seconds: u16) {
        if seconds == 0 {
            self.permit_until = None;
        } else {
            self.permit_until = Some(Instant::now() + Duration::from_secs(u64::from(seconds)));
            self.schedule_joins();
        }
    }

    fn may_join(&self, device: &SimDevice) -> bool {
        if !self.up || !self.permit_open() || !self.allow_joins {
            return false;
        }
        match device.install_code_key {
            Some(key) => self.join_keys.get(&device.ieee) == Some(&key),
            None => !self.install_code_only && self.well_known_ok,
        }
    }

    pub fn schedule_joins(&self) {
        for d in self.devices.values() {
            if d.pairing && !self.joined.contains_key(&d.ieee) {
                self.schedule(self.join_delay, Scheduled::Join(d.ieee));
            }
        }
    }

    pub fn join(&mut self, ieee: Ieee) {
        let Some(device) = self.devices.get(&ieee).cloned() else {
            return;
        };
        if self.joined.contains_key(&ieee) || !self.may_join(&device) {
            return;
        }
        let nwk = self.next_nwk;
        self.next_nwk = self.next_nwk.wrapping_add(0x1111);
        self.joined.insert(
            ieee,
            Joined {
                nwk,
                state: DeviceState::default(),
            },
        );
        tracing::info!(%ieee, nwk = format!("{nwk:#06x}"), "sim: device joined");
        self.schedule(
            Duration::ZERO,
            Scheduled::Mesh(MeshEvent::Joined {
                nwk,
                ieee,
                end_device: (!device.router).then_some(device.sleepy),
                key_exchange: !device.legacy,
            }),
        );
        let announce = device.announce(nwk);
        self.deliver(ieee, announce, Duration::from_millis(30));
    }

    pub fn leave_device(&mut self, ieee: Ieee) {
        if let Some(j) = self.joined.remove(&ieee) {
            self.schedule(
                Duration::from_millis(5),
                Scheduled::Mesh(MeshEvent::Left { nwk: j.nwk, ieee }),
            );
        }
    }

    /// A unicast from the coordinator. Returns whether a device took it.
    pub fn unicast(&mut self, destination: NodeId, out: &Outgoing) -> bool {
        let Some(ieee) = self.ieee_of(destination) else {
            return false;
        };
        let coordinator = self.ieee();
        let delay = self.reply_delay;
        let Some(mut device) = self.devices.get(&ieee).cloned() else {
            return false;
        };
        let Some(joined) = self.joined.get_mut(&ieee) else {
            return false;
        };
        let reaction = if out.profile == zigbee_zdo::PROFILE_ID && out.destination_endpoint == 0 {
            device.handle_zdo(
                destination,
                &mut joined.state,
                out.cluster,
                &out.payload,
                coordinator,
            )
        } else {
            device.handle_zcl(
                &mut joined.state,
                out.destination_endpoint,
                out.cluster,
                out.source_endpoint,
                &out.payload,
            )
        };
        self.devices.insert(ieee, device);
        for reply in reaction.replies {
            self.deliver(ieee, reply, delay);
        }
        if reaction.leave {
            self.leave_device(ieee);
        }
        true
    }

    /// Form (or restore) a network.
    pub fn form(&mut self, network: SimNetwork) {
        self.network = Some(network);
        self.up = true;
    }

    /// Leave the stored network; every device is gone with it.
    pub fn leave(&mut self) -> bool {
        self.up = false;
        self.joined.clear();
        self.network.take().is_some()
    }

    /// Move the network to another channel (a network update); every
    /// device follows. Returns false without a network.
    pub fn change_channel(&mut self, channel: u8) -> bool {
        match self.network.as_mut() {
            Some(n) => {
                n.channel = channel;
                n.nwk_update_id = n.nwk_update_id.wrapping_add(1);
                true
            }
            None => false,
        }
    }

    /// Energy per channel in the mask, dBm.
    pub fn energy(channel_mask: u32) -> Vec<(u8, i8)> {
        (11..=26u8)
            .filter(|c| channel_mask & (1 << c) != 0)
            .map(|c| (c, -95 + ((c * 7) % 30) as i8))
            .collect()
    }
}
