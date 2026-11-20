//! The ZNP front-end: a Z-Stack 3 coordinator over MT.
//!
//! Non-volatile items behave as Z-Stack's do where the host relies on them:
//! configuration items are inputs to formation, `EXTADDR` takes effect at
//! the next reset, and the network key, link key seed and frame counter
//! items read and write the stored network.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use zigbee_types::wire::Writer;
use zigbee_types::{ExtPanId, Ieee, Key};
use zigbee_znp::ids::{self, nv, nv_ex, product, startup_option, state, status};
use zigbee_znp::{Decoded, Decoder, Frame, IncomingMsg, Indication, Kind, Request};

use crate::device::Outgoing;
use crate::mesh::{Mesh, MeshEvent, Scheduled, SimNetwork};
use crate::ncp::{lock, SimError, Simulator};

/// ZDO cluster of a network update notify.
const MGMT_NWK_UPDATE_NOTIFY: u16 = 0x8038;
/// ZDO cluster of a device announce.
const DEVICE_ANNOUNCE: u16 = 0x0013;

pub(crate) struct ZnpFront {
    product: u8,
    nv: HashMap<u16, Vec<u8>>,
    nv_ex: HashMap<(u8, u16, u16), Vec<u8>>,
    extaddr_pending: Option<Ieee>,
    endpoints: Vec<(u8, u16)>,
    zdo_callbacks: bool,
    primary_mask: u32,
    state: u8,
    require_key_exchange: bool,
    pub link_keys: BTreeMap<Ieee, Key>,
}

fn status_only(s: u8) -> Vec<u8> {
    vec![s]
}

impl ZnpFront {
    pub fn new(product: u8) -> ZnpFront {
        ZnpFront {
            product,
            nv: HashMap::new(),
            nv_ex: HashMap::new(),
            extaddr_pending: None,
            endpoints: Vec::new(),
            zdo_callbacks: false,
            primary_mask: 0,
            state: state::HOLD,
            require_key_exchange: false,
            link_keys: BTreeMap::new(),
        }
    }

    fn reset(&mut self, mesh: &mut Mesh) {
        mesh.reset();
        self.endpoints.clear();
        self.zdo_callbacks = false;
        self.state = state::HOLD;
        if let Some(ieee) = self.extaddr_pending.take() {
            mesh.ieee_override = (ieee != mesh.factory_ieee).then_some(ieee);
        }
        let option = self
            .nv
            .get(&nv::STARTUP_OPTION)
            .and_then(|v| v.first().copied())
            .unwrap_or(0);
        if option & startup_option::CLEAR_STATE != 0 {
            mesh.leave();
            self.link_keys.clear();
            self.nv_ex.clear();
        }
        if option & startup_option::CLEAR_CONFIG != 0 {
            for id in [
                nv::PANID,
                nv::EXTENDED_PAN_ID,
                nv::APS_USE_EXT_PANID,
                nv::CHANLIST,
                nv::PRECFGKEY,
                nv::TCLK_SEED,
            ] {
                self.nv.remove(&id);
            }
        }
        self.nv.insert(nv::STARTUP_OPTION, vec![0]);
    }

    fn nv_item(&self, mesh: &Mesh, id: u16) -> Option<Vec<u8>> {
        match (id, &mesh.network) {
            (nv::EXTADDR, _) => Some(
                self.extaddr_pending
                    .unwrap_or_else(|| mesh.ieee())
                    .to_le_bytes()
                    .to_vec(),
            ),
            (nv::NWK_ACTIVE_KEY_INFO, Some(n)) => {
                let mut v = vec![n.key_sequence];
                v.put_key(&n.network_key);
                Some(v)
            }
            (nv::TCLK_SEED, Some(n)) => Some(n.tc_root.0.to_vec()),
            (nv::LEGACY_NWK_SEC_MATERIAL_TABLE_START, Some(n))
                if self.product == product::ZSTACK_30X =>
            {
                Some(sec_material(n))
            }
            _ => self.nv.get(&id).cloned(),
        }
    }

    fn write_nv(&mut self, mesh: &mut Mesh, id: u16, offset: usize, value: &[u8]) -> u8 {
        let mut item = self.nv_item(mesh, id).unwrap_or_default();
        if item.len() < offset + value.len() {
            item.resize(offset + value.len(), 0);
        }
        if let Some(dst) = item.get_mut(offset..offset + value.len()) {
            dst.copy_from_slice(value);
        }
        match (id, mesh.network.as_mut()) {
            (nv::EXTADDR, _) => {
                if let Ok(bytes) = <[u8; 8]>::try_from(item.as_slice()) {
                    self.extaddr_pending = Some(Ieee::from_le_bytes(bytes));
                }
            }
            (nv::NWK_ACTIVE_KEY_INFO, Some(n)) => {
                if let (Some(&seq), Some(key)) = (item.first(), item.get(1..17)) {
                    n.key_sequence = seq;
                    if let Ok(k) = <[u8; 16]>::try_from(key) {
                        n.network_key = Key(k);
                    }
                }
            }
            (nv::TCLK_SEED, Some(n)) => {
                if let Ok(k) = <[u8; 16]>::try_from(item.as_slice()) {
                    n.tc_root = Key(k);
                }
            }
            (nv::LEGACY_NWK_SEC_MATERIAL_TABLE_START, Some(n))
                if self.product == product::ZSTACK_30X =>
            {
                apply_sec_material(n, &item);
            }
            _ => {
                self.nv.insert(id, item);
            }
        }
        status::SUCCESS
    }

    fn nv_ex_item(&self, mesh: &Mesh, key: (u8, u16, u16)) -> Option<Vec<u8>> {
        match (key, &mesh.network) {
            ((nv_ex::SYSID_ZSTACK, nv_ex::NWK_SEC_MATERIAL_TABLE, 0), Some(n))
                if self.product == product::ZSTACK_3X0 =>
            {
                Some(sec_material(n))
            }
            _ => self.nv_ex.get(&key).cloned(),
        }
    }

    fn form(&mut self, mesh: &mut Mesh) -> u8 {
        let logical = self
            .nv
            .get(&nv::LOGICAL_TYPE)
            .and_then(|v| v.first().copied());
        if logical != Some(0) || mesh.network.is_some() {
            return status::FAILURE;
        }
        let u16_item = |id| {
            self.nv
                .get(&id)
                .and_then(|v| <[u8; 2]>::try_from(v.as_slice()).ok())
                .map(u16::from_le_bytes)
        };
        let u32_item = |id| {
            self.nv
                .get(&id)
                .and_then(|v| <[u8; 4]>::try_from(v.as_slice()).ok())
                .map(u32::from_le_bytes)
        };
        let key_item = |id| {
            self.nv
                .get(&id)
                .and_then(|v| <[u8; 16]>::try_from(v.as_slice()).ok())
                .map(Key)
        };
        let mask = if self.primary_mask != 0 {
            self.primary_mask
        } else {
            u32_item(nv::CHANLIST).unwrap_or(1 << 11)
        };
        let channel = (11..=26u8).find(|c| mask & (1 << c) != 0).unwrap_or(11);
        let extended_pan_id = self
            .nv
            .get(&nv::EXTENDED_PAN_ID)
            .and_then(|v| <[u8; 8]>::try_from(v.as_slice()).ok())
            .map(ExtPanId::from_le_bytes)
            .unwrap_or(ExtPanId(mesh.ieee().0));
        mesh.form(SimNetwork {
            channel,
            pan_id: u16_item(nv::PANID).unwrap_or(0x1A62),
            extended_pan_id,
            nwk_update_id: 0,
            channel_mask: mask,
            network_key: key_item(nv::PRECFGKEY).unwrap_or(Key([0x01; 16])),
            key_sequence: 0,
            tc_root: key_item(nv::TCLK_SEED).unwrap_or(Key([0x5A; 16])),
            frame_counter: 0,
        });
        for (delay, s) in [(5, state::COORD_STARTING), (10, state::ZB_COORD)] {
            mesh.schedule(
                Duration::from_millis(delay),
                Scheduled::Znp(Indication::StateChange { state: s }),
            );
        }
        self.state = state::ZB_COORD;
        mesh.schedule(
            Duration::from_millis(15),
            Scheduled::Znp(Indication::Commissioning {
                status: 0,
                mode: ids::commissioning::FORMATION,
                remaining: 0,
            }),
        );
        status::SUCCESS
    }

    fn profile_of(&self, endpoint: u8) -> u16 {
        self.endpoints
            .iter()
            .find(|(e, _)| *e == endpoint)
            .map(|(_, p)| *p)
            .unwrap_or(0x0104)
    }

    /// Handle a synchronous request; returns the response data.
    fn request(&mut self, mesh: &mut Mesh, request: Request) -> Vec<u8> {
        use Request::*;
        match request {
            SysReset { .. } => Vec::new(),
            SysPing => vec![0x59, 0x06],
            SysVersion => {
                let mut v = vec![2, self.product, 2, 7, 1];
                v.put_u32(20_230_507);
                v
            }
            OsalNvItemInit { id, init, .. } => {
                if self.nv_item(mesh, id).is_some() {
                    status_only(status::SUCCESS)
                } else {
                    self.write_nv(mesh, id, 0, &init);
                    status_only(status::NV_ITEM_UNINIT)
                }
            }
            OsalNvRead { id, offset } => match self.nv_item(mesh, id) {
                Some(item) => {
                    let mut v = vec![status::SUCCESS];
                    let chunk = item.get(usize::from(offset)..).unwrap_or(&[]);
                    v.put_u8_prefixed(chunk.get(..chunk.len().min(240)).unwrap_or(chunk));
                    v
                }
                None => vec![status::NV_OPER_FAILED, 0],
            },
            OsalNvWrite { id, offset, value } => {
                if self.nv_item(mesh, id).is_none() {
                    return status_only(status::NV_ITEM_UNINIT);
                }
                status_only(self.write_nv(mesh, id, usize::from(offset), &value))
            }
            OsalNvDelete { id, .. } => {
                self.nv.remove(&id);
                status_only(status::SUCCESS)
            }
            OsalNvLength { id } => {
                let len = self.nv_item(mesh, id).map(|v| v.len()).unwrap_or(0);
                (len as u16).to_le_bytes().to_vec()
            }
            NvLength { sysid, item, sub } => {
                let len = self
                    .nv_ex_item(mesh, (sysid, item, sub))
                    .map(|v| v.len())
                    .unwrap_or(0);
                (len as u32).to_le_bytes().to_vec()
            }
            NvRead {
                sysid,
                item,
                sub,
                offset,
                len,
            } => match self.nv_ex_item(mesh, (sysid, item, sub)) {
                Some(value) => {
                    let start = usize::from(offset).min(value.len());
                    let end = (start + usize::from(len)).min(value.len());
                    let mut v = vec![status::SUCCESS];
                    v.put_u8_prefixed(value.get(start..end).unwrap_or(&[]));
                    v
                }
                None => vec![status::NV_OPER_FAILED, 0],
            },
            NvWrite {
                sysid,
                item,
                sub,
                offset,
                value,
            } => {
                let key = (sysid, item, sub);
                let Some(mut current) = self.nv_ex_item(mesh, key) else {
                    return status_only(status::NV_OPER_FAILED);
                };
                let offset = usize::from(offset);
                if current.len() < offset + value.len() {
                    current.resize(offset + value.len(), 0);
                }
                if let Some(dst) = current.get_mut(offset..offset + value.len()) {
                    dst.copy_from_slice(&value);
                }
                match (key, mesh.network.as_mut()) {
                    ((nv_ex::SYSID_ZSTACK, nv_ex::NWK_SEC_MATERIAL_TABLE, 0), Some(n))
                        if self.product == product::ZSTACK_3X0 =>
                    {
                        apply_sec_material(n, &current)
                    }
                    _ => {
                        self.nv_ex.insert(key, current);
                    }
                }
                status_only(status::SUCCESS)
            }
            SetTxPower { dbm } => vec![dbm as u8],
            UtilGetDeviceInfo => {
                let mut v = vec![status::SUCCESS];
                v.put_ieee(mesh.ieee());
                v.put_u16(if mesh.up { 0x0000 } else { 0xFFFE });
                v.put_u8(0x07);
                v.put_u8(self.state);
                v.put_u8(0);
                v
            }
            ZdoSetLinkKey { ieee, key, .. } => {
                self.link_keys.insert(ieee, key);
                status_only(status::SUCCESS)
            }
            ZdoSendData {
                destination,
                tsn,
                cluster,
                data,
            } => {
                if !mesh.up {
                    return status_only(status::FAILURE);
                }
                if destination == 0x0000 && cluster == 0x0038 {
                    return self.nwk_update_to_self(mesh, &data);
                }
                let mut payload = vec![tsn];
                payload.extend_from_slice(&data);
                if destination >= 0xFFF8 {
                    // A broadcast: a channel change reaches every device.
                    if cluster == 0x0038 {
                        return self.nwk_update_to_self(mesh, &data);
                    }
                    return status_only(status::SUCCESS);
                }
                let out = Outgoing {
                    profile: 0,
                    cluster,
                    source_endpoint: 0,
                    destination_endpoint: 0,
                    payload,
                };
                mesh.unicast(destination, &out);
                status_only(status::SUCCESS)
            }
            ZdoMgmtPermitJoin { duration, .. } => {
                if !mesh.up {
                    return status_only(status::FAILURE);
                }
                mesh.allow_joins = duration > 0;
                mesh.well_known_ok = !mesh.install_code_only;
                mesh.permit(u16::from(duration));
                mesh.schedule(
                    Duration::ZERO,
                    Scheduled::Znp(Indication::PermitJoin { duration }),
                );
                status_only(status::SUCCESS)
            }
            ZdoMgmtNwkUpdate {
                destination,
                channel_mask,
                scan_duration,
                ..
            } => {
                let mut data = Vec::new();
                data.put_u32(channel_mask);
                data.put_u8(scan_duration);
                if destination == 0x0000 || destination >= 0xFFF8 {
                    let _ = self.nwk_update_to_self(mesh, &data);
                }
                status_only(status::SUCCESS)
            }
            ZdoMsgCbRegister { cluster } => {
                if cluster == 0xFFFF {
                    self.zdo_callbacks = true;
                }
                status_only(status::SUCCESS)
            }
            ZdoStartupFromApp { .. } => {
                if mesh.network.is_some() {
                    mesh.up = true;
                    self.state = state::ZB_COORD;
                    mesh.schedule(
                        Duration::from_millis(5),
                        Scheduled::Znp(Indication::StateChange {
                            state: state::ZB_COORD,
                        }),
                    );
                    vec![0]
                } else {
                    vec![2]
                }
            }
            ZdoExtNwkInfo => {
                let mut v = Vec::new();
                v.put_u16(if mesh.up { 0 } else { 0xFFFE });
                v.put_u8(self.state);
                let (pan, ext, channel) = match (&mesh.network, mesh.up) {
                    (Some(n), true) => (n.pan_id, n.extended_pan_id, n.channel),
                    _ => (0xFFFF, ExtPanId(0), 0),
                };
                v.put_u16(pan);
                v.put_u16(0xFFFE);
                v.put_ext_pan_id(ext);
                v.put_u64(0);
                v.put_u8(channel);
                v
            }
            AfRegister {
                endpoint, profile, ..
            } => {
                if self.endpoints.iter().any(|(e, _)| *e == endpoint) {
                    return status_only(0xB8);
                }
                self.endpoints.push((endpoint, profile));
                status_only(status::SUCCESS)
            }
            AfDataRequest {
                destination,
                destination_endpoint,
                source_endpoint,
                cluster,
                trans_id,
                data,
                ..
            } => {
                if !mesh.up {
                    return status_only(status::FAILURE);
                }
                let out = Outgoing {
                    profile: self.profile_of(source_endpoint),
                    cluster,
                    source_endpoint,
                    destination_endpoint,
                    payload: data,
                };
                let delivered = mesh.unicast(destination, &out);
                mesh.schedule(
                    Duration::from_millis(2),
                    Scheduled::Znp(Indication::DataConfirm {
                        status: if delivered {
                            status::SUCCESS
                        } else {
                            status::MAC_NO_ACK
                        },
                        endpoint: source_endpoint,
                        trans_id,
                    }),
                );
                status_only(status::SUCCESS)
            }
            AfDataRequestExt {
                source_endpoint,
                trans_id,
                ..
            } => {
                if !mesh.up {
                    return status_only(status::FAILURE);
                }
                mesh.schedule(
                    Duration::from_millis(2),
                    Scheduled::Znp(Indication::DataConfirm {
                        status: status::SUCCESS,
                        endpoint: source_endpoint,
                        trans_id,
                    }),
                );
                status_only(status::SUCCESS)
            }
            SetAllowRejoinTcPolicy { .. } => status_only(status::SUCCESS),
            BdbSetTcRequireKeyExchange { required } => {
                self.require_key_exchange = required;
                status_only(status::SUCCESS)
            }
            BdbSetJoinUsesInstallCodeKey { required } => {
                mesh.install_code_only = required;
                mesh.well_known_ok = !required;
                status_only(status::SUCCESS)
            }
            BdbAddInstallCode { format, ieee, code } => {
                match (format, <[u8; 16]>::try_from(code.as_slice())) {
                    (2, Ok(key)) => {
                        mesh.join_keys.insert(ieee, Key(key));
                        if mesh.permit_open() {
                            mesh.schedule_joins();
                        }
                        status_only(status::SUCCESS)
                    }
                    _ => status_only(status::FAILURE),
                }
            }
            BdbStartCommissioning { mode } => {
                if mode & ids::commissioning::FORMATION != 0 {
                    status_only(self.form(mesh))
                } else {
                    status_only(status::SUCCESS)
                }
            }
            BdbSetChannel {
                primary,
                channel_mask,
            } => {
                if primary {
                    self.primary_mask = channel_mask;
                }
                status_only(status::SUCCESS)
            }
        }
    }

    /// A Mgmt_NWK_Update_req the coordinator handles itself: an energy
    /// scan (duration 0 to 5) answered with a notify, or a channel change
    /// (0xFE). `data` is the request after the sequence number.
    fn nwk_update_to_self(&mut self, mesh: &mut Mesh, data: &[u8]) -> Vec<u8> {
        let (Some(mask), Some(&duration)) = (
            data.get(..4)
                .and_then(|b| <[u8; 4]>::try_from(b).ok())
                .map(u32::from_le_bytes),
            data.get(4),
        ) else {
            return status_only(status::FAILURE);
        };
        if duration == 0xFE {
            if let Some(channel) = (11..=26u8).find(|c| mask & (1 << c) != 0) {
                mesh.change_channel(channel);
            }
        } else if duration <= 5 {
            let energies = Mesh::energy(mask);
            let mut notify = vec![0];
            notify.put_u16(100);
            notify.put_u16(0);
            notify.put_u32(mask);
            notify.put_u8(energies.len() as u8);
            for (_, dbm) in energies {
                notify.put_u8(dbm_to_ed(dbm));
            }
            if self.zdo_callbacks {
                mesh.schedule(
                    Duration::from_millis(30),
                    Scheduled::Znp(Indication::ZdoMessage {
                        source: 0x0000,
                        broadcast: false,
                        cluster: MGMT_NWK_UPDATE_NOTIFY,
                        secured: false,
                        tsn: data.get(5).copied().unwrap_or(0),
                        mac_destination: 0x0000,
                        data: notify,
                    }),
                );
            }
        }
        status_only(status::SUCCESS)
    }

    fn translate(&self, event: MeshEvent) -> Vec<Indication> {
        match event {
            MeshEvent::Joined { nwk, ieee, .. } => vec![Indication::TcDevice {
                nwk,
                ieee,
                parent: 0x0000,
            }],
            MeshEvent::Left { nwk, ieee } => vec![Indication::Leave {
                nwk,
                ieee,
                request: false,
                remove_children: false,
                rejoin: false,
            }],
            MeshEvent::Incoming {
                nwk,
                ieee,
                lqi,
                aps_seq,
                out,
                ..
            } => {
                if out.profile == 0 && out.destination_endpoint == 0 {
                    let mut v = Vec::new();
                    if out.cluster == DEVICE_ANNOUNCE {
                        v.push(Indication::DeviceAnnounce {
                            source: nwk,
                            nwk,
                            ieee,
                            capabilities: out.payload.get(11).copied().unwrap_or(0),
                        });
                    }
                    if self.zdo_callbacks {
                        v.push(Indication::ZdoMessage {
                            source: nwk,
                            broadcast: out.cluster == DEVICE_ANNOUNCE,
                            cluster: out.cluster,
                            secured: false,
                            tsn: out.payload.first().copied().unwrap_or(0),
                            mac_destination: 0x0000,
                            data: out.payload.get(1..).unwrap_or(&[]).to_vec(),
                        });
                    }
                    v
                } else {
                    vec![Indication::Incoming(Box::new(IncomingMsg {
                        group: 0,
                        cluster: out.cluster,
                        source: nwk,
                        source_ieee: None,
                        source_endpoint: out.source_endpoint,
                        destination_endpoint: out.destination_endpoint,
                        broadcast: false,
                        lqi,
                        secured: false,
                        timestamp: 0,
                        tsn: aps_seq,
                        data: out.payload,
                        last_hop: Some(nwk),
                        extended: false,
                    }))]
                }
            }
        }
    }
}

/// An energy reading in 802.15.4 ED units, as Z-Stack reports them.
fn dbm_to_ed(dbm: i8) -> u8 {
    ((i16::from(dbm) + 100).clamp(0, 40) * 255 / 40) as u8
}

fn sec_material(n: &SimNetwork) -> Vec<u8> {
    let mut v = Vec::new();
    v.put_u32(n.frame_counter);
    v.put_ext_pan_id(n.extended_pan_id);
    v
}

fn apply_sec_material(n: &mut SimNetwork, item: &[u8]) {
    if let Some(fc) = item
        .get(..4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(u32::from_le_bytes)
    {
        n.frame_counter = fc;
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
    lock(&sim.state).mesh.sink = Some(action_tx.clone());
    let mut decoder = Decoder::new();
    let mut buf = [0u8; 256];
    let result = 'serve: loop {
        let frames: Vec<Frame> = tokio::select! {
            n = read.read(&mut buf) => {
                let n = match n {
                    Ok(0) => break Err(SimError::Closed("host closed the port".into())),
                    Ok(n) => n,
                    Err(e) => break Err(SimError::Closed(e.to_string())),
                };
                let mut out = Vec::new();
                for decoded in decoder.push(buf.get(..n).unwrap_or(&[])) {
                    let Decoded::Frame(frame) = decoded else { continue };
                    let mut s = lock(&sim.state);
                    let s = &mut *s;
                    let Some(front) = s.front.znp() else { continue };
                    match Request::decode(&frame) {
                        Ok(Request::SysReset { .. }) => {
                            front.reset(&mut s.mesh);
                            out.push(
                                Indication::Reset {
                                    reason: 1,
                                    transport: 2,
                                    product: front.product,
                                    major: 2,
                                    minor: 7,
                                    hw: 1,
                                }
                                .encode(),
                            );
                        }
                        Ok(request) => {
                            tracing::trace!(?request, "sim: znp request");
                            let data = front.request(&mut s.mesh, request);
                            out.push(Frame::new(Kind::Srsp, frame.subsystem, frame.id, data));
                        }
                        Err(_) if frame.kind == Kind::Sreq => {
                            // RPC error: invalid command.
                            out.push(Frame::new(
                                Kind::Srsp,
                                ids::subsystem::RPC_ERROR,
                                0,
                                vec![2, frame.command().0, frame.id],
                            ));
                        }
                        Err(_) => {}
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
                match action {
                    Scheduled::Join(ieee) => {
                        s.mesh.join(ieee);
                        continue;
                    }
                    Scheduled::Remove(ieee) => {
                        s.mesh.leave_device(ieee);
                        continue;
                    }
                    Scheduled::Znp(ind) => vec![ind.encode()],
                    Scheduled::Mesh(event) => match s.front.znp() {
                        Some(front) => {
                            if let MeshEvent::Joined { ieee, key_exchange: false, .. } = event {
                                if front.require_key_exchange {
                                    // Z-Stack removes a joiner that never
                                    // asks for its own link key.
                                    s.mesh.schedule(Duration::from_millis(300), Scheduled::Remove(ieee));
                                }
                            }
                            front.translate(event).iter().map(Indication::encode).collect()
                        }
                        None => continue,
                    },
                    Scheduled::Ezsp(_) => continue,
                }
            }
        };
        for frame in frames {
            let Ok(bytes) = frame.encode() else { continue };
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
