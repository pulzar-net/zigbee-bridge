//! The [`Adapter`] for Texas Instruments Z-Stack 3 coordinators over ZNP.
//!
//! Z-Stack runs the trust center itself: joiners get the network key under
//! the well-known key (or their install code key), then a unique link key
//! that Z-Stack derives from a random seed the host writes before forming.
//! The seed is in every backup. ZDO frames go both ways raw
//! (`ZDO_SEND_DATA` with the host's sequence number, and every incoming ZDO
//! frame forwarded), so the host's ZDO code is the same as on EZSP.
//!
//! What Z-Stack does not report, the adapter leaves out rather than guess:
//! signal strength (only link quality), the route record originator's IEEE
//! address, delivery of ZDO frames (reported as sent once queued), and the
//! network update id (read as 0; restores keep the backup's value in the
//! backup only).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot, watch};
use zigbee_primitives::wire::{Reader, Writer};
use zigbee_primitives::{ExtPanId, Ieee, Key, NodeId};
use zigbee_znp::client::{ClientConfig, Znp, ZnpError, ZnpEvent};
use zigbee_znp::ids::{
    addr_mode, af_option, commissioning, nv, nv_ex, product, startup_option, state, status,
};
use zigbee_znp::{Indication, Request};

use crate::adapter::{
    Adapter, AdapterEvent, AdapterInfo, Aps, Beacon, JoinPolicy, NetworkInfo, NetworkSettings,
    StartMode, StartOutcome,
};
use crate::backup::{
    self, Backup, BackupDevice, Internal, Metadata, NetworkKey, StackSpecific, ZstackSpecific,
};
use crate::error::HostError;

/// How long the stack may take to come up or form.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a reset may take.
const RESET_TIMEOUT: Duration = Duration::from_secs(5);
/// ZDO cluster of a network update notify.
const MGMT_NWK_UPDATE_NOTIFY: u16 = 0x8038;
/// Every ZDO cluster, for `ZDO_MSG_CB_REGISTER`.
const ALL_ZDO_CLUSTERS: u16 = 0xFFFF;
/// The profile endpoint 1 serves.
const HOME_AUTOMATION: u16 = 0x0104;

/// The clusters the host's endpoint 1 serves and uses.
const ENDPOINT_INPUT_CLUSTERS: &[u16] = &[
    0x0000, 0x0003, 0x0006, 0x000A, 0x0019, 0x0300, 0x0402, 0x0405, 0x0500, 0x0702, 0x0B04,
];
const ENDPOINT_OUTPUT_CLUSTERS: &[u16] = &[
    0x0000, 0x0001, 0x0003, 0x0004, 0x0005, 0x0006, 0x0008, 0x000F, 0x0012, 0x0020, 0x0101, 0x0102,
    0x0201, 0x0202, 0x0300, 0x0400, 0x0402, 0x0403, 0x0405, 0x0406, 0x0500, 0x0502, 0x0702, 0x0B04,
    0x0B05,
];

type ScanResult = Result<Vec<(u8, i8)>, HostError>;
type Discovery = (Vec<Beacon>, oneshot::Sender<Result<Vec<Beacon>, HostError>>);

struct Shared {
    /// Device state from `ZDO_STATE_CHANGE_IND`.
    state: watch::Sender<u8>,
    /// Reset indications.
    resets: watch::Sender<u32>,
    /// Commissioning results: (mode, status).
    commissioning: watch::Sender<Option<(u8, u8)>>,
    /// Registered endpoints and their profiles.
    endpoints: Mutex<HashMap<u8, u16>>,
    /// An energy scan waiting for its notify.
    scan: Mutex<Option<oneshot::Sender<ScanResult>>>,
    /// An active scan: beacons so far, and who waits for the end.
    discovery: Mutex<Option<Discovery>>,
}

/// A Z-Stack 3 coordinator.
pub struct ZnpAdapter {
    znp: Znp,
    shared: Arc<Shared>,
    tag: AtomicU8,
    /// For delivery reports the adapter makes itself.
    events: mpsc::UnboundedSender<AdapterEvent>,
}

impl std::fmt::Debug for ZnpAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZnpAdapter")
            .field("version", &self.znp.version())
            .finish_non_exhaustive()
    }
}

impl From<ZnpError> for HostError {
    fn from(e: ZnpError) -> Self {
        match e {
            ZnpError::Down(why) => HostError::Ncp(format!("ZNP port down: {why}")),
            other => HostError::Ncp(other.to_string()),
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn random_key() -> Result<Key, HostError> {
    let mut k = [0u8; 16];
    getrandom::fill(&mut k).map_err(|e| HostError::Io(format!("random: {e}")))?;
    Ok(Key(k))
}

/// An 802.15.4 energy reading as an approximate dBm figure: Z-Stack's
/// scale spans 40 dB above about -100 dBm.
fn ed_to_dbm(ed: u8) -> i8 {
    (-100 + i16::from(ed) * 40 / 255) as i8
}

fn translate(ind: Indication, shared: &Shared) -> Option<AdapterEvent> {
    match ind {
        Indication::Reset { .. } => {
            shared.resets.send_modify(|n| *n = n.wrapping_add(1));
            None
        }
        Indication::StateChange { state: s } => {
            let was = *shared.state.borrow();
            shared.state.send_replace(s);
            match (was == state::ZB_COORD, s == state::ZB_COORD) {
                (false, true) => Some(AdapterEvent::NetworkUp),
                (true, false) => Some(AdapterEvent::NetworkDown),
                _ => None,
            }
        }
        Indication::Commissioning { status, mode, .. } => {
            // 1 is "in progress".
            if status != 1 {
                shared.commissioning.send_replace(Some((mode, status)));
            }
            None
        }
        Indication::Incoming(m) => {
            let profile = shared
                .endpoints
                .lock()
                .ok()
                .and_then(|e| e.get(&m.destination_endpoint).copied())
                .unwrap_or(HOME_AUTOMATION);
            Some(AdapterEvent::Message {
                source: m.source,
                source_ieee: m.source_ieee,
                aps: Aps {
                    profile,
                    cluster: m.cluster,
                    source_endpoint: m.source_endpoint,
                    destination_endpoint: m.destination_endpoint,
                },
                broadcast: m.broadcast || m.group != 0,
                group: (m.group != 0).then_some(m.group),
                lqi: m.lqi,
                rssi: None,
                payload: m.data,
            })
        }
        Indication::ZdoMessage {
            source,
            broadcast,
            cluster,
            tsn,
            data,
            ..
        } => {
            if cluster == MGMT_NWK_UPDATE_NOTIFY && source == 0x0000 {
                if let Some(done) = shared.scan.lock().ok().and_then(|mut s| s.take()) {
                    let _ = done.send(parse_scan_notify(&data));
                    return None;
                }
            }
            let mut payload = Vec::with_capacity(data.len() + 1);
            payload.push(tsn);
            payload.extend_from_slice(&data);
            Some(AdapterEvent::Message {
                source,
                source_ieee: None,
                aps: Aps {
                    profile: zigbee_zdo::PROFILE_ID,
                    cluster,
                    source_endpoint: 0,
                    destination_endpoint: 0,
                },
                broadcast,
                group: None,
                lqi: 0,
                rssi: None,
                payload,
            })
        }
        Indication::TcDevice { nwk, ieee, parent } => Some(AdapterEvent::Joined {
            nwk,
            ieee,
            parent,
            rejoin: false,
        }),
        Indication::Leave {
            nwk, ieee, rejoin, ..
        } => (!rejoin).then_some(AdapterEvent::Left { nwk, ieee }),
        Indication::SourceRoute { source, relays } => Some(AdapterEvent::RouteRecord {
            source,
            ieee: None,
            relays,
            lqi: None,
            rssi: None,
        }),
        Indication::DataConfirm {
            status: s,
            trans_id,
            ..
        } => Some(AdapterEvent::Delivery {
            tag: u16::from(trans_id),
            delivered: s == status::SUCCESS,
        }),
        Indication::Beacons(beacons) => {
            if let Ok(mut d) = shared.discovery.lock() {
                if let Some((found, _)) = d.as_mut() {
                    for b in beacons
                        .into_iter()
                        .take(256usize.saturating_sub(found.len()))
                    {
                        found.push(Beacon {
                            channel: b.channel,
                            pan_id: b.pan_id,
                            extended_pan_id: b.extended_pan_id,
                            permit_joining: b.permit_joining,
                            nwk_update_id: b.update_id,
                            lqi: b.lqi,
                            rssi: None,
                        });
                    }
                }
            }
            None
        }
        Indication::DiscoveryDone { status: s } => {
            if let Some((found, done)) = shared.discovery.lock().ok().and_then(|mut d| d.take()) {
                let _ = done.send(if s == status::SUCCESS {
                    Ok(found)
                } else {
                    Err(HostError::Ncp(format!(
                        "network discovery failed ({s:#04x})"
                    )))
                });
            }
            None
        }
        // The raw announce arrives as a forwarded ZDO frame as well.
        Indication::DeviceAnnounce { .. } | Indication::PermitJoin { .. } => None,
    }
}

fn parse_scan_notify(data: &[u8]) -> ScanResult {
    let mut r = Reader::new(data);
    let parse = |r: &mut Reader<'_>| -> Result<Vec<(u8, i8)>, zigbee_primitives::wire::Truncated> {
        let status = r.u8()?;
        if status != 0 {
            return Ok(Vec::new());
        }
        let _total = r.u16()?;
        let _failures = r.u16()?;
        let mask = r.u32()?;
        let count = usize::from(r.u8()?);
        let energies = r.bytes(count)?;
        Ok((11..=26u8)
            .filter(|c| mask & (1 << c) != 0)
            .zip(energies.iter())
            .map(|(c, ed)| (c, ed_to_dbm(*ed)))
            .collect())
    };
    parse(&mut r).map_err(|e| HostError::Ncp(format!("network update notify: {e}")))
}

impl ZnpAdapter {
    /// Connect to a Z-Stack coordinator over `io`. Returns the adapter and
    /// its events.
    pub async fn connect<T>(
        io: T,
        config: ClientConfig,
    ) -> Result<(ZnpAdapter, mpsc::UnboundedReceiver<AdapterEvent>), HostError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (znp, mut znp_events) = Znp::start(io, config).await?;
        let v = znp.version();
        tracing::info!(product = v.product, firmware = %v.firmware(), "Z-Stack coordinator connected");
        let shared = Arc::new(Shared {
            state: watch::channel(state::HOLD).0,
            resets: watch::channel(0).0,
            commissioning: watch::channel(None).0,
            endpoints: Mutex::new(HashMap::new()),
            scan: Mutex::new(None),
            discovery: Mutex::new(None),
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let events = tx.clone();
        let task_shared = shared.clone();
        tokio::spawn(async move {
            while let Some(event) = znp_events.recv().await {
                let out = match event {
                    ZnpEvent::Indication(ind) => translate(ind, &task_shared),
                    ZnpEvent::Down(why) => Some(AdapterEvent::Failed(why)),
                };
                if let Some(out) = out {
                    if tx.send(out).is_err() {
                        break;
                    }
                }
            }
        });
        Ok((
            ZnpAdapter {
                znp,
                shared,
                tag: AtomicU8::new(1),
                events,
            },
            rx,
        ))
    }

    /// The firmware's version.
    pub fn version(&self) -> zigbee_znp::client::Version {
        self.znp.version()
    }

    async fn ok(&self, request: Request) -> Result<(), HostError> {
        self.znp.request_ok(request).await?;
        Ok(())
    }

    /// Soft-reset the stack and wait for it to come back.
    async fn reset(&self) -> Result<(), HostError> {
        let mut resets = self.shared.resets.subscribe();
        self.shared.state.send_replace(state::HOLD);
        self.ok(Request::SysReset { soft: true }).await?;
        match tokio::time::timeout(RESET_TIMEOUT, resets.changed()).await {
            Ok(Ok(())) => {}
            _ => {
                return Err(HostError::Ncp(
                    "the coordinator did not come back from a reset".into(),
                ))
            }
        }
        if let Ok(mut e) = self.shared.endpoints.lock() {
            e.clear();
        }
        Ok(())
    }

    async fn wait_coordinator(&self) -> Result<(), HostError> {
        let mut s = self.shared.state.subscribe();
        let up = tokio::time::timeout(NETWORK_TIMEOUT, s.wait_for(|s| *s == state::ZB_COORD))
            .await
            .is_ok_and(|r| r.is_ok());
        if up {
            Ok(())
        } else {
            Err(HostError::Ncp("the network did not come up".into()))
        }
    }

    /// Start the stack on whatever network it has stored. Returns whether
    /// it had one.
    async fn startup(&self) -> Result<bool, HostError> {
        let data = self
            .znp
            .request(Request::ZdoStartupFromApp { delay: 100 })
            .await?;
        // 0: restored a network, 1: new network, 2: not started.
        if data.first() == Some(&0) {
            self.wait_coordinator().await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn nwk_info(&self) -> Result<(NodeId, u16, ExtPanId, u8), HostError> {
        let data = self.znp.request(Request::ZdoExtNwkInfo).await?;
        let mut r = Reader::new(&data);
        let parse = |r: &mut Reader<'_>| -> Result<_, zigbee_primitives::wire::Truncated> {
            let nwk = r.u16()?;
            let _state = r.u8()?;
            let pan = r.u16()?;
            let _parent = r.u16()?;
            let ext = r.ext_pan_id()?;
            let _parent_ext = r.u64()?;
            let channel = r.u8()?;
            Ok((nwk, pan, ext, channel))
        };
        parse(&mut r).map_err(|e| HostError::Ncp(format!("ZDO_EXT_NWK_INFO: {e}")))
    }

    async fn ieee(&self) -> Result<Ieee, HostError> {
        let data = self.znp.request_ok(Request::UtilGetDeviceInfo).await?;
        let mut r = Reader::new(&data);
        let _ = r.u8();
        r.ieee()
            .map_err(|e| HostError::Ncp(format!("UTIL_GET_DEVICE_INFO: {e}")))
    }

    async fn network_key(&self) -> Result<(u8, Key), HostError> {
        let item = self
            .znp
            .nv_read(nv::NWK_ACTIVE_KEY_INFO)
            .await?
            .ok_or_else(|| HostError::Ncp("no active network key".into()))?;
        let mut r = Reader::new(&item);
        let seq = r.u8().map_err(|e| HostError::Ncp(e.to_string()))?;
        let key = r.key().map_err(|e| HostError::Ncp(e.to_string()))?;
        Ok((seq, key))
    }

    fn sec_material_id(&self) -> SecMaterial {
        if self.znp.version().product == product::ZSTACK_3X0 {
            SecMaterial::Extended
        } else {
            SecMaterial::Legacy
        }
    }

    async fn frame_counter(&self) -> Result<u32, HostError> {
        let item = match self.sec_material_id() {
            SecMaterial::Extended => {
                self.znp
                    .nv_ex_read(nv_ex::SYSID_ZSTACK, nv_ex::NWK_SEC_MATERIAL_TABLE, 0)
                    .await?
            }
            SecMaterial::Legacy => {
                self.znp
                    .nv_read(nv::LEGACY_NWK_SEC_MATERIAL_TABLE_START)
                    .await?
            }
        };
        let item = item.ok_or_else(|| HostError::Ncp("no network frame counter".into()))?;
        Reader::new(&item)
            .u32()
            .map_err(|e| HostError::Ncp(format!("network security material: {e}")))
    }

    async fn set_frame_counter(&self, counter: u32, ext: ExtPanId) -> Result<(), HostError> {
        let mut item = Vec::new();
        item.put_u32(counter);
        item.put_ext_pan_id(ext);
        match self.sec_material_id() {
            SecMaterial::Extended => {
                self.znp
                    .nv_ex_write(nv_ex::SYSID_ZSTACK, nv_ex::NWK_SEC_MATERIAL_TABLE, 0, &item)
                    .await?
            }
            SecMaterial::Legacy => {
                self.znp
                    .nv_write(nv::LEGACY_NWK_SEC_MATERIAL_TABLE_START, &item)
                    .await?
            }
        }
        Ok(())
    }

    /// Clear the stored network and configuration, then form `settings`.
    async fn form(&self, settings: &NetworkSettings, seed: Key) -> Result<(), HostError> {
        self.znp
            .nv_write(
                nv::STARTUP_OPTION,
                &[startup_option::CLEAR_STATE | startup_option::CLEAR_CONFIG],
            )
            .await?;
        self.reset().await?;
        let mask = 1u32 << settings.channel;
        self.znp.nv_write(nv::LOGICAL_TYPE, &[0]).await?;
        self.znp.nv_write(nv::PRECFGKEYS_ENABLE, &[0]).await?;
        self.znp.nv_write(nv::ZDO_DIRECT_CB, &[1]).await?;
        self.znp.nv_write(nv::CHANLIST, &mask.to_le_bytes()).await?;
        self.znp
            .nv_write(nv::PANID, &settings.pan_id.to_le_bytes())
            .await?;
        let ext = settings.extended_pan_id.to_le_bytes();
        self.znp.nv_write(nv::EXTENDED_PAN_ID, &ext).await?;
        self.znp.nv_write(nv::APS_USE_EXT_PANID, &ext).await?;
        self.znp
            .nv_write(nv::PRECFGKEY, &settings.network_key.0)
            .await?;
        self.znp.nv_write(nv::TCLK_SEED, &seed.0).await?;
        self.ok(Request::BdbSetChannel {
            primary: true,
            channel_mask: mask,
        })
        .await?;
        self.ok(Request::BdbSetChannel {
            primary: false,
            channel_mask: 0,
        })
        .await?;
        let mut done = self.shared.commissioning.subscribe();
        self.shared.commissioning.send_replace(None);
        self.ok(Request::BdbStartCommissioning {
            mode: commissioning::FORMATION,
        })
        .await?;
        let result = tokio::time::timeout(
            NETWORK_TIMEOUT,
            done.wait_for(|r| matches!(r, Some((m, _)) if *m & commissioning::FORMATION != 0)),
        )
        .await
        .ok()
        .and_then(|r| r.ok().map(|r| *r));
        match result {
            Some(Some((_, 0))) => {}
            Some(r) => {
                return Err(HostError::Ncp(format!(
                    "network formation failed (status {:?})",
                    r.map(|(_, s)| s)
                )))
            }
            None => return Err(HostError::Ncp("network formation did not finish".into())),
        }
        self.wait_coordinator().await
    }

    async fn restore(&self, backup: &Backup) -> Result<(), HostError> {
        let ieee = self.ieee().await?;
        if ieee != backup.coordinator_ieee {
            // Z-Stack reads its IEEE address from NV at start-up.
            self.znp
                .nv_write(nv::EXTADDR, &backup.coordinator_ieee.to_le_bytes())
                .await?;
            self.reset().await?;
            let now = self.ieee().await?;
            if now != backup.coordinator_ieee {
                return Err(HostError::RestoreRefused(format!(
                    "this coordinator kept its IEEE address {now}; the backup needs {}",
                    backup.coordinator_ieee
                )));
            }
        }
        let seed = backup
            .stack_specific
            .as_ref()
            .and_then(|s| s.zstack.as_ref())
            .map(|z| z.tclk_seed);
        let settings = NetworkSettings {
            channel: backup.channel,
            pan_id: backup.pan_id,
            extended_pan_id: backup.extended_pan_id,
            network_key: backup.network_key.key,
            tx_power: None,
        };
        self.form(&settings, seed.map_or_else(random_key, Ok)?)
            .await?;
        self.set_frame_counter(backup.restore_frame_counter(), backup.extended_pan_id)
            .await?;
        if backup.network_key.sequence_number != 0 {
            let mut item = vec![backup.network_key.sequence_number];
            item.put_key(&backup.network_key.key);
            self.znp.nv_write(nv::NWK_ACTIVE_KEY_INFO, &item).await?;
        }
        if seed.is_none() {
            // Keys derived by another stack: install each one.
            for d in &backup.devices {
                if let Some(link) = &d.link_key {
                    self.ok(Request::ZdoSetLinkKey {
                        nwk: d.nwk_address.unwrap_or(0xFFFE),
                        ieee: d.ieee_address,
                        key: link.key,
                    })
                    .await?;
                }
            }
        }
        Ok(())
    }

    async fn register_endpoint(&self, endpoint: u8, profile: u16) -> Result<(), HostError> {
        let known = self
            .shared
            .endpoints
            .lock()
            .ok()
            .and_then(|e| e.get(&endpoint).copied());
        match known {
            Some(p) if p == profile => return Ok(()),
            Some(p) => {
                return Err(HostError::Invalid(format!(
                    "endpoint {endpoint} is registered for profile {p:#06x}, not {profile:#06x}"
                )))
            }
            None => {}
        }
        let (inputs, outputs) = if endpoint == 1 {
            (
                ENDPOINT_INPUT_CLUSTERS.to_vec(),
                ENDPOINT_OUTPUT_CLUSTERS.to_vec(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        self.ok(Request::AfRegister {
            endpoint,
            profile,
            device_id: 0x0005,
            version: 0,
            input_clusters: inputs,
            output_clusters: outputs,
        })
        .await?;
        if let Ok(mut e) = self.shared.endpoints.lock() {
            e.insert(endpoint, profile);
        }
        Ok(())
    }

    async fn after_up(&self, power: Option<i8>) -> Result<NetworkInfo, HostError> {
        self.ok(Request::ZdoMsgCbRegister {
            cluster: ALL_ZDO_CLUSTERS,
        })
        .await?;
        self.register_endpoint(1, HOME_AUTOMATION).await?;
        self.ok(Request::SetAllowRejoinTcPolicy { allow: true })
            .await?;
        if let Some(dbm) = power {
            if let Err(e) = self.znp.request(Request::SetTxPower { dbm }).await {
                tracing::warn!(dbm, error = %e, "setting transmit power");
            }
        }
        self.read_network_info().await
    }

    async fn read_network_info(&self) -> Result<NetworkInfo, HostError> {
        let (nwk, pan_id, extended_pan_id, channel) = self.nwk_info().await?;
        Ok(NetworkInfo {
            ieee: self.ieee().await?,
            nwk,
            channel,
            pan_id,
            extended_pan_id,
            nwk_update_id: 0,
        })
    }

    fn next_tag(&self) -> u8 {
        self.tag.fetch_add(1, Ordering::Relaxed)
    }
}

enum SecMaterial {
    Extended,
    Legacy,
}

impl Adapter for ZnpAdapter {
    fn info(&self) -> AdapterInfo {
        let v = self.znp.version();
        AdapterInfo {
            kind: "znp",
            firmware: v.firmware(),
            protocol: v.transport,
            reports_key_exchange: false,
        }
    }

    async fn start(&self, mode: StartMode) -> Result<(StartOutcome, NetworkInfo), HostError> {
        self.reset().await?;
        self.znp.nv_write(nv::ZDO_DIRECT_CB, &[1]).await?;
        let has_network = self.startup().await?;
        match mode {
            StartMode::Restore(backup) => {
                self.restore(&backup).await?;
                Ok((StartOutcome::Restored, self.after_up(None).await?))
            }
            StartMode::Normal {
                settings,
                replace_other,
            } => {
                if has_network {
                    let (_, pan, ext, channel) = self.nwk_info().await?;
                    let (_, key) = self.network_key().await?;
                    if pan == settings.pan_id
                        && ext == settings.extended_pan_id
                        && key == settings.network_key
                    {
                        return Ok((
                            StartOutcome::Resumed,
                            self.after_up(settings.tx_power).await?,
                        ));
                    }
                    let found = format!("channel {channel}, PAN {pan:04x}, extended PAN {ext}");
                    if !replace_other {
                        return Err(HostError::NetworkMismatch { found });
                    }
                    tracing::warn!(%found, "replacing the coordinator's other network, as configured");
                }
                self.form(&settings, random_key()?).await?;
                Ok((
                    StartOutcome::Formed,
                    self.after_up(settings.tx_power).await?,
                ))
            }
        }
    }

    async fn permit_join(&self, seconds: u8, join: &JoinPolicy) -> Result<(), HostError> {
        if seconds > 0 {
            self.ok(Request::BdbSetJoinUsesInstallCodeKey {
                required: join.install_code_only,
            })
            .await?;
            self.ok(Request::BdbSetTcRequireKeyExchange {
                required: !join.allow_legacy,
            })
            .await?;
        }
        for (mode, destination) in [(addr_mode::SHORT, 0x0000), (addr_mode::BROADCAST, 0xFFFC)] {
            self.ok(Request::ZdoMgmtPermitJoin {
                addr_mode: mode,
                destination,
                duration: seconds,
                tc_significance: 0,
            })
            .await?;
        }
        Ok(())
    }

    async fn add_join_key(&self, ieee: Ieee, key: Key) -> Result<(), HostError> {
        // Format 2: the key derived from the install code.
        self.ok(Request::BdbAddInstallCode {
            format: 2,
            ieee,
            code: key.0.to_vec(),
        })
        .await
    }

    async fn send_unicast(
        &self,
        nwk: NodeId,
        aps: Aps,
        payload: Vec<u8>,
    ) -> Result<u16, HostError> {
        let tag = self.next_tag();
        if aps.profile == zigbee_zdo::PROFILE_ID && aps.destination_endpoint == 0 {
            let (tsn, data) = payload
                .split_first()
                .ok_or_else(|| HostError::Invalid("empty ZDO frame".into()))?;
            self.ok(Request::ZdoSendData {
                destination: nwk,
                tsn: *tsn,
                cluster: aps.cluster,
                data: data.to_vec(),
            })
            .await?;
            // Z-Stack confirms a ZDO frame only as queued; report that.
            let _ = self.events.send(AdapterEvent::Delivery {
                tag: u16::from(tag),
                delivered: true,
            });
            return Ok(u16::from(tag));
        }
        self.register_endpoint(aps.source_endpoint, aps.profile)
            .await?;
        self.ok(Request::AfDataRequest {
            destination: nwk,
            destination_endpoint: aps.destination_endpoint,
            source_endpoint: aps.source_endpoint,
            cluster: aps.cluster,
            trans_id: tag,
            options: af_option::ACK_REQUEST | af_option::DISCOVER_ROUTE,
            radius: 30,
            data: payload,
        })
        .await?;
        Ok(u16::from(tag))
    }

    async fn send_broadcast(
        &self,
        destination: NodeId,
        aps: Aps,
        payload: Vec<u8>,
    ) -> Result<(), HostError> {
        if aps.profile == zigbee_zdo::PROFILE_ID && aps.destination_endpoint == 0 {
            let (tsn, data) = payload
                .split_first()
                .ok_or_else(|| HostError::Invalid("empty ZDO frame".into()))?;
            return self
                .ok(Request::ZdoSendData {
                    destination,
                    tsn: *tsn,
                    cluster: aps.cluster,
                    data: data.to_vec(),
                })
                .await;
        }
        self.register_endpoint(aps.source_endpoint, aps.profile)
            .await?;
        self.ok(Request::AfDataRequestExt {
            addr_mode: addr_mode::BROADCAST,
            destination: u64::from(destination),
            destination_endpoint: aps.destination_endpoint,
            destination_pan: 0,
            source_endpoint: aps.source_endpoint,
            cluster: aps.cluster,
            trans_id: self.next_tag(),
            options: 0,
            radius: 30,
            data: payload,
        })
        .await
    }

    async fn network_info(&self) -> Result<NetworkInfo, HostError> {
        self.read_network_info().await
    }

    async fn backup(&self) -> Result<Backup, HostError> {
        let (_, pan_id, extended_pan_id, channel) = self.nwk_info().await?;
        let (sequence_number, key) = self.network_key().await?;
        let frame_counter = self.frame_counter().await?;
        let seed = self
            .znp
            .nv_read(nv::TCLK_SEED)
            .await?
            .and_then(|v| <[u8; 16]>::try_from(v.as_slice()).ok())
            .map(Key);
        let v = self.znp.version();
        Ok(Backup {
            metadata: Metadata {
                format: backup::FORMAT.into(),
                version: 1,
                source: concat!("zigbee-bridge@", env!("CARGO_PKG_VERSION")).into(),
                internal: Internal {
                    created_unix: Some(now_unix()),
                    ezsp_version: None,
                    firmware: Some(v.firmware()),
                },
            },
            stack_specific: seed.map(|tclk_seed| StackSpecific {
                ezsp: None,
                zstack: Some(ZstackSpecific { tclk_seed }),
            }),
            coordinator_ieee: self.ieee().await?,
            pan_id,
            extended_pan_id,
            nwk_update_id: 0,
            security_level: 5,
            channel,
            channel_mask: vec![channel],
            network_key: NetworkKey {
                key,
                sequence_number,
                frame_counter,
            },
            devices: Vec::<BackupDevice>::new(),
        })
    }

    async fn active_scan(&self, channel_mask: u32) -> Result<Vec<Beacon>, HostError> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut d) = self.shared.discovery.lock() {
            if d.is_some() {
                return Err(HostError::Invalid("a scan is already running".into()));
            }
            *d = Some((Vec::new(), tx));
        }
        let sent = self
            .ok(Request::ZdoNwkDiscovery {
                channel_mask,
                scan_duration: 3,
            })
            .await;
        if let Err(e) = sent {
            if let Ok(mut d) = self.shared.discovery.lock() {
                *d = None;
            }
            return Err(e);
        }
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(result)) => result,
            _ => {
                if let Ok(mut d) = self.shared.discovery.lock() {
                    *d = None;
                }
                Err(HostError::Ncp("network discovery did not complete".into()))
            }
        }
    }

    async fn leave(&self) -> Result<(), HostError> {
        // Z-Stack forgets its network and configuration at the next reset.
        self.znp
            .nv_write(
                nv::STARTUP_OPTION,
                &[startup_option::CLEAR_STATE | startup_option::CLEAR_CONFIG],
            )
            .await?;
        self.reset().await
    }

    async fn energy_scan(&self, channel_mask: u32) -> Result<Vec<(u8, i8)>, HostError> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut s) = self.shared.scan.lock() {
            if s.is_some() {
                return Err(HostError::Invalid("a scan is already running".into()));
            }
            *s = Some(tx);
        }
        let sent = self
            .ok(Request::ZdoMgmtNwkUpdate {
                destination: 0x0000,
                addr_mode: addr_mode::SHORT,
                channel_mask,
                scan_duration: 3,
                scan_count: 1,
                manager: 0x0000,
            })
            .await;
        if let Err(e) = sent {
            if let Ok(mut s) = self.shared.scan.lock() {
                *s = None;
            }
            return Err(e);
        }
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(result)) => result,
            _ => {
                if let Ok(mut s) = self.shared.scan.lock() {
                    *s = None;
                }
                Err(HostError::Ncp("energy scan did not complete".into()))
            }
        }
    }
}
