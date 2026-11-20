//! The deCONZ front-end: a ConBee II over the deCONZ serial protocol.
//!
//! As the firmware does, it queues received frames and delivery confirms
//! and announces them through the device state, which the host polls for.
//! Network parameters are inputs to formation while offline and read back
//! the running network while connected.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use zigbee_deconz::ids::{device_state, net_state, param, status};
use zigbee_deconz::{Address, ApsConfirm, ApsIndication, Decoder, Request, Response};
use zigbee_types::{ExtPanId, Ieee, Key};

use crate::device::Outgoing;
use crate::mesh::{Mesh, MeshEvent, Scheduled, SimNetwork};
use crate::ncp::{lock, SimError, Simulator};

/// The firmware version the simulator reports (a ConBee II).
pub(crate) const FIRMWARE: u32 = 0x2678_0700;
/// ZDO cluster of a network update notify.
const MGMT_NWK_UPDATE_NOTIFY: u16 = 0x8038;

/// A frame for the host that is not a reply.
#[derive(Debug)]
pub(crate) enum Pending {
    /// Tell the host the device state changed.
    StateChanged,
}

pub(crate) struct DeconzFront {
    params: HashMap<u8, Vec<u8>>,
    indications: VecDeque<ApsIndication>,
    confirms: VecDeque<ApsConfirm>,
    pub link_keys: BTreeMap<Ieee, Key>,
}

fn u16_of(v: Option<&Vec<u8>>) -> Option<u16> {
    v.and_then(|v| <[u8; 2]>::try_from(v.as_slice()).ok())
        .map(u16::from_le_bytes)
}

fn u32_of(v: Option<&Vec<u8>>) -> Option<u32> {
    v.and_then(|v| <[u8; 4]>::try_from(v.as_slice()).ok())
        .map(u32::from_le_bytes)
}

fn u64_of(v: Option<&Vec<u8>>) -> Option<u64> {
    v.and_then(|v| <[u8; 8]>::try_from(v.as_slice()).ok())
        .map(u64::from_le_bytes)
}

impl DeconzFront {
    pub fn new() -> DeconzFront {
        DeconzFront {
            params: HashMap::new(),
            indications: VecDeque::new(),
            confirms: VecDeque::new(),
            link_keys: BTreeMap::new(),
        }
    }

    fn state(&self, mesh: &Mesh) -> u8 {
        let mut s = if mesh.up {
            net_state::CONNECTED
        } else {
            net_state::OFFLINE
        };
        if !self.confirms.is_empty() {
            s |= device_state::CONFIRM_PENDING;
        }
        if !self.indications.is_empty() {
            s |= device_state::INDICATION_PENDING;
        }
        s | device_state::REQUEST_FREE
    }

    fn reset(&mut self, mesh: &mut Mesh) {
        // A new connection: the firmware keeps running.
        self.indications.clear();
        self.confirms.clear();
        let _ = mesh;
    }

    fn read(&self, mesh: &Mesh, p: u8, extra: &[u8]) -> Option<Vec<u8>> {
        let net = mesh.network.as_ref().filter(|_| mesh.up);
        Some(match p {
            param::MAC_ADDRESS => mesh.ieee().to_le_bytes().to_vec(),
            param::NWK_PANID => match net {
                Some(n) => n.pan_id.to_le_bytes().to_vec(),
                None => self.params.get(&p).cloned().unwrap_or(vec![0xFF, 0xFF]),
            },
            param::NWK_ADDRESS => (if mesh.up { 0u16 } else { 0xFFFE }).to_le_bytes().to_vec(),
            param::NWK_EXTENDED_PANID => net
                .map(|n| n.extended_pan_id.to_le_bytes())
                .unwrap_or([0; 8])
                .to_vec(),
            param::CURRENT_CHANNEL => vec![net.map(|n| n.channel).unwrap_or(0)],
            param::NWK_UPDATE_ID => match net {
                Some(n) => vec![n.nwk_update_id],
                None => self.params.get(&p).cloned().unwrap_or(vec![0]),
            },
            param::NWK_FRAME_COUNTER => match net {
                Some(n) => n.frame_counter.to_le_bytes().to_vec(),
                None => self.params.get(&p).cloned().unwrap_or(vec![0; 4]),
            },
            param::NETWORK_KEY => {
                let key = match net {
                    Some(n) => n.network_key.0.to_vec(),
                    None => self
                        .params
                        .get(&p)
                        .and_then(|v| v.get(1..).map(<[u8]>::to_vec))
                        .unwrap_or(vec![0; 16]),
                };
                let mut v = vec![extra.first().copied().unwrap_or(0)];
                v.extend_from_slice(&key);
                v
            }
            param::LINK_KEY => {
                let ieee = Ieee::from_le_bytes(extra.get(..8)?.try_into().ok()?);
                let key = self.link_keys.get(&ieee)?;
                let mut v = ieee.to_le_bytes().to_vec();
                v.extend_from_slice(&key.0);
                v
            }
            param::PROTOCOL_VERSION => 0x0108u16.to_le_bytes().to_vec(),
            param::PERMIT_JOIN => vec![u8::from(mesh.permit_open())],
            // Unset parameters read as the firmware's zero defaults.
            _ => match self.params.get(&p) {
                Some(v) => v.clone(),
                None => match p {
                    param::APS_DESIGNED_COORDINATOR
                    | param::SECURITY_MODE
                    | param::USE_PREDEFINED_NWK_PANID => vec![0],
                    param::CHANNEL_MASK | param::WATCHDOG_TTL => vec![0; 4],
                    param::APS_EXTENDED_PANID | param::TRUST_CENTER_ADDRESS => vec![0; 8],
                    param::APP_ZDP_RESPONSE_HANDLING => vec![0; 2],
                    _ => return None,
                },
            },
        })
    }

    fn write(&mut self, mesh: &mut Mesh, p: u8, value: &[u8]) -> u8 {
        match p {
            param::MAC_ADDRESS => {
                let Ok(bytes) = <[u8; 8]>::try_from(value) else {
                    return status::INVALID_VALUE;
                };
                let ieee = Ieee::from_le_bytes(bytes);
                mesh.ieee_override = (ieee != mesh.factory_ieee).then_some(ieee);
            }
            param::PERMIT_JOIN => {
                let seconds = value.first().copied().unwrap_or(0);
                if !mesh.up {
                    return status::NO_NETWORK;
                }
                mesh.allow_joins = seconds > 0;
                mesh.well_known_ok = true;
                mesh.permit(u16::from(seconds));
            }
            param::LINK_KEY => {
                let (Some(ieee), Some(key)) = (
                    value
                        .get(..8)
                        .and_then(|b| <[u8; 8]>::try_from(b).ok())
                        .map(Ieee::from_le_bytes),
                    value.get(8..24).and_then(|b| <[u8; 16]>::try_from(b).ok()),
                ) else {
                    return status::INVALID_VALUE;
                };
                self.link_keys.insert(ieee, Key(key));
                if ieee != mesh.ieee() {
                    mesh.join_keys.insert(ieee, Key(key));
                    if mesh.permit_open() {
                        mesh.schedule_joins();
                    }
                }
            }
            param::NWK_FRAME_COUNTER => {
                let Ok(bytes) = <[u8; 4]>::try_from(value) else {
                    return status::INVALID_VALUE;
                };
                if let Some(n) = mesh.network.as_mut() {
                    n.frame_counter = u32::from_le_bytes(bytes);
                }
                self.params.insert(p, value.to_vec());
            }
            param::APS_DESIGNED_COORDINATOR => {
                // No longer a coordinator, while offline: the network is
                // gone.
                if value.first() == Some(&0) && !mesh.up {
                    mesh.leave();
                }
                self.params.insert(p, value.to_vec());
            }
            param::NWK_UPDATE_ID => {
                if let (Some(n), Some(id)) = (mesh.network.as_mut(), value.first()) {
                    if mesh.up {
                        n.nwk_update_id = *id;
                    }
                }
                self.params.insert(p, value.to_vec());
            }
            _ => {
                self.params.insert(p, value.to_vec());
            }
        }
        status::SUCCESS
    }

    /// The network the parameters describe.
    fn configured(&self, mesh: &Mesh) -> Option<SimNetwork> {
        let designed = self.params.get(&param::APS_DESIGNED_COORDINATOR)?.first() == Some(&1);
        if !designed {
            return None;
        }
        let mask = u32_of(self.params.get(&param::CHANNEL_MASK)).unwrap_or(1 << 11);
        let key = self
            .params
            .get(&param::NETWORK_KEY)
            .and_then(|v| v.get(1..17))
            .and_then(|k| <[u8; 16]>::try_from(k).ok())
            .map(Key)?;
        Some(SimNetwork {
            channel: (11..=26u8).find(|c| mask & (1 << c) != 0).unwrap_or(11),
            pan_id: u16_of(self.params.get(&param::NWK_PANID)).unwrap_or(0x1A62),
            extended_pan_id: ExtPanId(
                u64_of(self.params.get(&param::APS_EXTENDED_PANID)).unwrap_or(mesh.ieee().0),
            ),
            nwk_update_id: self
                .params
                .get(&param::NWK_UPDATE_ID)
                .and_then(|v| v.first().copied())
                .unwrap_or(0),
            channel_mask: mask,
            network_key: key,
            key_sequence: 0,
            tc_root: Key([0; 16]),
            frame_counter: u32_of(self.params.get(&param::NWK_FRAME_COUNTER)).unwrap_or(0),
        })
    }

    fn change_state(&mut self, mesh: &mut Mesh, state: u8) -> u8 {
        match state {
            net_state::OFFLINE => {
                mesh.up = false;
                mesh.permit_until = None;
            }
            net_state::CONNECTED => {
                if mesh.up {
                    return status::SUCCESS;
                }
                // Parameters written since going offline form a new
                // network; unchanged ones resume the stored one.
                let wanted = self.configured(mesh);
                let same = match (&mesh.network, &wanted) {
                    (Some(n), Some(w)) => {
                        n.pan_id == w.pan_id
                            && n.extended_pan_id == w.extended_pan_id
                            && n.network_key == w.network_key
                    }
                    (Some(_), None) => true,
                    _ => false,
                };
                if same {
                    mesh.up = true;
                } else if let Some(w) = wanted {
                    mesh.leave();
                    mesh.form(w);
                } else {
                    return status::FAILURE;
                }
                // Values written before formation now belong to the network.
                self.params.remove(&param::NWK_FRAME_COUNTER);
            }
            _ => return status::INVALID_VALUE,
        }
        mesh.schedule(
            Duration::from_millis(5),
            Scheduled::Deconz(Pending::StateChanged),
        );
        status::SUCCESS
    }

    fn request(&mut self, mesh: &mut Mesh, request: Request) -> (Response, u8) {
        let ok = status::SUCCESS;
        match request {
            Request::Version => (Response::Version { version: FIRMWARE }, ok),
            Request::ReadParameter { param: p, extra } => match self.read(mesh, p, &extra) {
                Some(value) => (Response::ReadParameter { param: p, value }, ok),
                None => (
                    Response::ReadParameter {
                        param: p,
                        value: vec![],
                    },
                    status::UNSUPPORTED,
                ),
            },
            Request::WriteParameter { param: p, value } => {
                let s = self.write(mesh, p, &value);
                (Response::WriteParameter { param: p }, s)
            }
            Request::DeviceState => (
                Response::DeviceState {
                    device_state: self.state(mesh),
                },
                ok,
            ),
            Request::ChangeNetworkState { state } => {
                let s = self.change_state(mesh, state);
                (Response::ChangeNetworkState { state }, s)
            }
            Request::ApsDataRequest(a) => {
                if !mesh.up {
                    return (
                        Response::ApsDataRequest {
                            device_state: self.state(mesh),
                            request_id: a.request_id,
                        },
                        status::NO_NETWORK,
                    );
                }
                let mut delivered = true;
                match a.destination {
                    Address::Nwk(0x0000) if a.profile == 0 && a.cluster == 0x0038 => {
                        self.network_update(mesh, &a.asdu);
                    }
                    Address::Nwk(n) if n >= 0xFFF8 => {
                        if a.profile == 0 && a.cluster == 0x0038 {
                            self.network_update(mesh, &a.asdu);
                        }
                    }
                    Address::Nwk(n) | Address::NwkIeee(n, _) => {
                        delivered = mesh.unicast(
                            n,
                            &Outgoing {
                                profile: a.profile,
                                cluster: a.cluster,
                                source_endpoint: a.source_endpoint,
                                destination_endpoint: a.destination_endpoint.unwrap_or(0),
                                payload: a.asdu.clone(),
                            },
                        );
                    }
                    Address::Ieee(i) => {
                        delivered = match mesh.nwk_of(i) {
                            Some(n) => mesh.unicast(
                                n,
                                &Outgoing {
                                    profile: a.profile,
                                    cluster: a.cluster,
                                    source_endpoint: a.source_endpoint,
                                    destination_endpoint: a.destination_endpoint.unwrap_or(0),
                                    payload: a.asdu.clone(),
                                },
                            ),
                            None => false,
                        };
                    }
                    Address::Group(_) => {}
                }
                self.confirms.push_back(ApsConfirm {
                    device_state: 0,
                    request_id: a.request_id,
                    destination: a.destination,
                    destination_endpoint: a.destination_endpoint,
                    source_endpoint: a.source_endpoint,
                    status: if delivered { 0x00 } else { 0xE9 },
                });
                mesh.schedule(
                    Duration::from_millis(2),
                    Scheduled::Deconz(Pending::StateChanged),
                );
                (
                    Response::ApsDataRequest {
                        device_state: self.state(mesh),
                        request_id: a.request_id,
                    },
                    ok,
                )
            }
            Request::ApsDataConfirm => match self.confirms.pop_front() {
                Some(mut c) => {
                    c.device_state = self.state(mesh);
                    (Response::ApsDataConfirm(Box::new(c)), ok)
                }
                None => (
                    Response::DeviceState {
                        device_state: self.state(mesh),
                    },
                    status::FAILURE,
                ),
            },
            Request::ApsDataIndication => match self.indications.pop_front() {
                Some(mut i) => {
                    i.device_state = self.state(mesh);
                    (Response::ApsDataIndication(Box::new(i)), ok)
                }
                None => (
                    Response::DeviceState {
                        device_state: self.state(mesh),
                    },
                    status::FAILURE,
                ),
            },
        }
    }

    fn network_update(&mut self, mesh: &mut Mesh, asdu: &[u8]) {
        let tsn = asdu.first().copied().unwrap_or(0);
        if let Some(notify) = mesh.network_update(asdu.get(1..).unwrap_or(&[])) {
            let mut payload = vec![tsn];
            payload.extend_from_slice(&notify);
            self.indications.push_back(ApsIndication {
                device_state: 0,
                destination: Address::Nwk(0),
                destination_endpoint: 0,
                source: Address::Nwk(0),
                source_endpoint: 0,
                profile: 0,
                cluster: MGMT_NWK_UPDATE_NOTIFY,
                asdu: payload,
                lqi: Some(255),
                rssi: Some(0),
            });
        }
    }

    /// Queue what the network sent; returns whether the host should hear
    /// of a new device state.
    fn translate(&mut self, event: MeshEvent) -> bool {
        match event {
            // The firmware reports neither joins nor leaves; the host
            // learns of a device from its announce.
            MeshEvent::Joined { .. } | MeshEvent::Left { .. } => false,
            MeshEvent::Incoming {
                nwk,
                ieee,
                lqi,
                rssi,
                out,
                ..
            } => {
                self.indications.push_back(ApsIndication {
                    device_state: 0,
                    destination: Address::Nwk(0),
                    destination_endpoint: out.destination_endpoint,
                    source: Address::NwkIeee(nwk, ieee),
                    source_endpoint: out.source_endpoint,
                    profile: out.profile,
                    cluster: out.cluster,
                    asdu: out.payload,
                    lqi: Some(lqi),
                    rssi: Some(rssi),
                });
                true
            }
        }
    }
}

/// Serve one host connection over `io` until it ends.
pub(crate) async fn serve<T>(sim: &Simulator, io: T) -> Result<(), SimError>
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut read, mut write) = tokio::io::split(io);
    let (action_tx, mut actions) = mpsc::unbounded_channel::<(Duration, Scheduled)>();
    let (due_tx, mut due) = mpsc::unbounded_channel::<Scheduled>();
    {
        let mut s = lock(&sim.state);
        let s = &mut *s;
        s.mesh.sink = Some(action_tx.clone());
        if let Some(front) = s.front.deconz() {
            front.reset(&mut s.mesh);
        }
    }
    let mut decoder = Decoder::new();
    let mut buf = [0u8; 256];
    let mut notice_seq: u8 = 0;
    let result = 'serve: loop {
        let frames: Vec<Vec<u8>> = tokio::select! {
            n = read.read(&mut buf) => {
                let n = match n {
                    Ok(0) => break Err(SimError::Closed("host closed the port".into())),
                    Ok(n) => n,
                    Err(e) => break Err(SimError::Closed(e.to_string())),
                };
                let mut out = Vec::new();
                for frame in decoder.push(buf.get(..n).unwrap_or(&[])) {
                    let Ok(frame) = frame else { continue };
                    let mut s = lock(&sim.state);
                    let s = &mut *s;
                    let Some(front) = s.front.deconz() else { continue };
                    match Request::decode(&frame) {
                        Ok(request) => {
                            tracing::trace!(?request, "sim: deconz request");
                            let (response, st) = front.request(&mut s.mesh, request);
                            // A failure echoes the command with no payload.
                            let reply = if st == status::SUCCESS {
                                response.encode(frame.seq, st)
                            } else {
                                zigbee_deconz::Frame::new(frame.command, frame.seq, st, vec![])
                            };
                            out.push(reply.encode());
                        }
                        Err(_) => out.push(
                            zigbee_deconz::Frame::new(frame.command, frame.seq, status::UNSUPPORTED, vec![]).encode(),
                        ),
                    }
                }
                out
            }
            Some((delay, action)) = actions.recv() => {
                let due_tx = due_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let _ = due_tx.send(action);
                });
                continue;
            }
            Some(action) = due.recv() => {
                let mut s = lock(&sim.state);
                let s = &mut *s;
                let notify = match action {
                    Scheduled::Join(ieee) => {
                        s.mesh.join(ieee);
                        false
                    }
                    Scheduled::Remove(ieee) => {
                        s.mesh.leave_device(ieee);
                        false
                    }
                    Scheduled::Deconz(Pending::StateChanged) => true,
                    Scheduled::Mesh(event) => match s.front.deconz() {
                        Some(front) => front.translate(event),
                        None => false,
                    },
                    Scheduled::Ezsp(_) | Scheduled::Znp(_) => false,
                };
                if !notify {
                    continue;
                }
                let Some(front) = s.front.deconz() else { continue };
                notice_seq = notice_seq.wrapping_add(1);
                vec![Response::DeviceStateChanged { device_state: front.state(&s.mesh) }
                    .encode(notice_seq, status::SUCCESS)
                    .encode()]
            }
        };
        for bytes in frames {
            if write.write_all(&bytes).await.is_err() {
                break 'serve Err(SimError::Closed("write failed".into()));
            }
        }
    };
    let mut state = lock(&sim.state);
    if state
        .mesh
        .sink
        .as_ref()
        .is_some_and(|a| a.same_channel(&action_tx))
    {
        state.mesh.sink = None;
    }
    result
}
