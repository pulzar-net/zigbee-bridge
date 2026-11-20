//! The [`Adapter`] for dresden elektronik ConBee and RaspBee coordinators
//! over the deCONZ serial protocol.
//!
//! The firmware queues received frames and delivery confirms and says so in
//! its device state; the adapter reads them whenever a frame reports one
//! waiting, and asks for the state every few seconds in case a notice was
//! lost. Network settings are parameters: the adapter takes the network
//! offline, writes them, and brings it back up.
//!
//! The firmware reports no joins or leaves (the host learns of a device from
//! its announce), keeps one global trust center link key rather than a key
//! per device, and cannot restrict joining to install codes; the adapter
//! refuses `install_code_only` rather than pretend.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot, watch};
use zigbee_deconz::client::{ClientConfig, Deconz, DeconzError, DeconzEvent};
use zigbee_deconz::ids::{device_state, net_state, param, security_mode, status, tx_option};
use zigbee_deconz::{Address, ApsRequest, Request, Response};
use zigbee_primitives::wire::Reader;
use zigbee_primitives::{ExtPanId, Ieee, Key, NodeId};

use crate::adapter::{
    Adapter, AdapterEvent, AdapterInfo, Aps, Beacon, JoinPolicy, NetworkInfo, NetworkSettings,
    StartMode, StartOutcome,
};
use crate::backup::{self, Backup, Internal, Metadata, NetworkKey};
use crate::error::HostError;

/// How long the network may take to come up or go down.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(20);
/// How often to ask for the device state regardless.
const STATE_POLL: Duration = Duration::from_secs(5);
/// The firmware watchdog: it resets the stick unless refreshed.
const WATCHDOG_TTL: u32 = 600;
const WATCHDOG_REFRESH: Duration = Duration::from_secs(240);
/// ZDO cluster of a network update notify.
const MGMT_NWK_UPDATE_NOTIFY: u16 = 0x8038;

type ScanResult = Result<Vec<(u8, i8)>, HostError>;

struct Shared {
    /// The network state bits.
    net: watch::Sender<u8>,
    scan: Mutex<Option<oneshot::Sender<ScanResult>>>,
}

/// A ConBee or RaspBee.
pub struct DeconzAdapter {
    deconz: Deconz,
    shared: Arc<Shared>,
    tag: AtomicU8,
    protocol: u16,
    legacy_warned: AtomicBool,
}

impl std::fmt::Debug for DeconzAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeconzAdapter")
            .field("firmware", &self.deconz.version().text())
            .finish_non_exhaustive()
    }
}

impl From<DeconzError> for HostError {
    fn from(e: DeconzError) -> Self {
        HostError::Ncp(e.to_string())
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ed_to_dbm(ed: u8) -> i8 {
    (-100 + i16::from(ed) * 40 / 255) as i8
}

fn parse_scan_notify(data: &[u8]) -> ScanResult {
    let mut r = Reader::new(data);
    let parse = |r: &mut Reader<'_>| -> Result<Vec<(u8, i8)>, zigbee_primitives::wire::Truncated> {
        let _tsn = r.u8()?;
        if r.u8()? != 0 {
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

/// Read every queued frame and confirm the device state says is waiting.
async fn drain(
    deconz: &Deconz,
    mut state: u8,
    shared: &Shared,
    tx: &mpsc::UnboundedSender<AdapterEvent>,
) {
    // Bounded: a firmware that keeps saying "pending" cannot spin forever.
    for _ in 0..64 {
        shared.net.send_replace(state & device_state::NETWORK_MASK);
        let next = if state & device_state::INDICATION_PENDING != 0 {
            deconz.request(Request::ApsDataIndication).await
        } else if state & device_state::CONFIRM_PENDING != 0 {
            deconz.request(Request::ApsDataConfirm).await
        } else {
            return;
        };
        match next {
            Ok(Response::ApsDataIndication(i)) => {
                state = i.device_state;
                if i.cluster == MGMT_NWK_UPDATE_NOTIFY
                    && i.profile == zigbee_zdo::PROFILE_ID
                    && i.source.nwk() == Some(0x0000)
                {
                    if let Some(done) = shared.scan.lock().ok().and_then(|mut s| s.take()) {
                        let _ = done.send(parse_scan_notify(&i.asdu));
                        continue;
                    }
                }
                let broadcast = matches!(i.destination, Address::Group(_))
                    || i.destination.nwk().is_some_and(|n| n >= 0xFFF8);
                let _ = tx.send(AdapterEvent::Message {
                    source: i.source.nwk().unwrap_or(0xFFFE),
                    source_ieee: i.source.ieee(),
                    aps: Aps {
                        profile: i.profile,
                        cluster: i.cluster,
                        source_endpoint: i.source_endpoint,
                        destination_endpoint: i.destination_endpoint,
                    },
                    broadcast,
                    group: match i.destination {
                        Address::Group(g) => Some(g),
                        _ => None,
                    },
                    lqi: i.lqi.unwrap_or(0),
                    rssi: i.rssi,
                    payload: i.asdu,
                });
            }
            Ok(Response::ApsDataConfirm(c)) => {
                state = c.device_state;
                let _ = tx.send(AdapterEvent::Delivery {
                    tag: u16::from(c.request_id),
                    delivered: c.status == 0,
                });
            }
            // Nothing queued after all.
            Ok(_) | Err(DeconzError::Status { .. }) => return,
            Err(e) => {
                tracing::warn!(error = %e, "reading the deCONZ queue");
                return;
            }
        }
    }
}

impl DeconzAdapter {
    /// Connect to a ConBee or RaspBee over `io`. Returns the adapter and
    /// its events.
    pub async fn connect<T>(
        io: T,
        config: ClientConfig,
    ) -> Result<(DeconzAdapter, mpsc::UnboundedReceiver<AdapterEvent>), HostError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (deconz, mut events) = Deconz::start(io, config).await?;
        let v = deconz.version();
        tracing::info!(firmware = %v.text(), product = v.product(), "deCONZ coordinator connected");
        let protocol = deconz
            .read_parameter(param::PROTOCOL_VERSION, &[])
            .await
            .ok()
            .and_then(|v| <[u8; 2]>::try_from(v.as_slice()).ok())
            .map(u16::from_le_bytes)
            .unwrap_or(0);
        if let Err(e) = deconz
            .write_parameter(param::APP_ZDP_RESPONSE_HANDLING, &1u16.to_le_bytes())
            .await
        {
            tracing::debug!(error = %e, "ZDP response handling");
        }
        let shared = Arc::new(Shared {
            net: watch::channel(net_state::OFFLINE).0,
            scan: Mutex::new(None),
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let task = (deconz.clone(), shared.clone(), tx.clone());
        tokio::spawn(async move {
            let (deconz, shared, tx) = task;
            let mut tick = tokio::time::interval(STATE_POLL);
            let mut watchdog = tokio::time::interval(WATCHDOG_REFRESH);
            loop {
                tokio::select! {
                    event = events.recv() => match event {
                        Some(DeconzEvent::State(s)) => drain(&deconz, s, &shared, &tx).await,
                        Some(DeconzEvent::Notice(_)) => {}
                        Some(DeconzEvent::Down(why)) => {
                            let _ = tx.send(AdapterEvent::Failed(why));
                            return;
                        }
                        None => {
                            let _ = tx.send(AdapterEvent::Failed("deCONZ events ended".into()));
                            return;
                        }
                    },
                    _ = tick.tick() => {
                        if let Ok(Response::DeviceState { device_state }) = deconz.request(Request::DeviceState).await {
                            drain(&deconz, device_state, &shared, &tx).await;
                        }
                    }
                    _ = watchdog.tick() => {
                        let _ = deconz.write_parameter(param::WATCHDOG_TTL, &WATCHDOG_TTL.to_le_bytes()).await;
                    }
                }
                if tx.is_closed() {
                    return;
                }
            }
        });
        Ok((
            DeconzAdapter {
                deconz,
                shared,
                tag: AtomicU8::new(1),
                protocol,
                legacy_warned: AtomicBool::new(false),
            },
            rx,
        ))
    }

    async fn read(&self, p: u8) -> Result<Vec<u8>, HostError> {
        Ok(self.deconz.read_parameter(p, &[]).await?)
    }

    async fn read_u16(&self, p: u8) -> Result<u16, HostError> {
        let v = self.read(p).await?;
        Reader::new(&v)
            .u16()
            .map_err(|e| HostError::Ncp(format!("parameter {p:#04x}: {e}")))
    }

    async fn read_u64(&self, p: u8) -> Result<u64, HostError> {
        let v = self.read(p).await?;
        Reader::new(&v)
            .u64()
            .map_err(|e| HostError::Ncp(format!("parameter {p:#04x}: {e}")))
    }

    async fn write(&self, p: u8, value: &[u8]) -> Result<(), HostError> {
        Ok(self.deconz.write_parameter(p, value).await?)
    }

    async fn network_key(&self) -> Result<Key, HostError> {
        let v = self.deconz.read_parameter(param::NETWORK_KEY, &[0]).await?;
        v.get(1..17)
            .and_then(|k| <[u8; 16]>::try_from(k).ok())
            .map(Key)
            .ok_or_else(|| HostError::Ncp("no network key".into()))
    }

    async fn device_state(&self) -> Result<u8, HostError> {
        match self.deconz.request(Request::DeviceState).await? {
            Response::DeviceState { device_state } => Ok(device_state),
            other => Err(HostError::Ncp(format!("device state: {other:?}"))),
        }
    }

    async fn set_net_state(&self, want: u8) -> Result<(), HostError> {
        let mut net = self.shared.net.subscribe();
        self.deconz
            .request(Request::ChangeNetworkState { state: want })
            .await?;
        // The firmware announces the change; ask as well, in case it does
        // not.
        let deadline = tokio::time::Instant::now() + NETWORK_TIMEOUT;
        loop {
            let s = self.device_state().await? & device_state::NETWORK_MASK;
            self.shared.net.send_replace(s);
            if s == want {
                return Ok(());
            }
            let wait = tokio::time::timeout_at(deadline, net.changed()).await;
            if wait.is_err() {
                return Err(HostError::Ncp(format!(
                    "the network did not go {}",
                    if want == net_state::CONNECTED {
                        "up"
                    } else {
                        "down"
                    }
                )));
            }
        }
    }

    /// Whether the stored parameters describe `settings`.
    async fn holds(&self, settings: &NetworkSettings) -> Result<bool, HostError> {
        let designed = self.read(param::APS_DESIGNED_COORDINATOR).await?.first() == Some(&1);
        let up = self.device_state().await? & device_state::NETWORK_MASK == net_state::CONNECTED;
        let ext = if up {
            self.read_u64(param::NWK_EXTENDED_PANID).await?
        } else {
            self.read_u64(param::APS_EXTENDED_PANID).await?
        };
        Ok(designed
            && self.read_u16(param::NWK_PANID).await? == settings.pan_id
            && ExtPanId(ext) == settings.extended_pan_id
            && self.network_key().await.ok() == Some(settings.network_key))
    }

    async fn form(
        &self,
        settings: &NetworkSettings,
        restore: Option<&Backup>,
    ) -> Result<(), HostError> {
        self.set_net_state(net_state::OFFLINE).await?;
        if let Some(b) = restore {
            self.write(param::MAC_ADDRESS, &b.coordinator_ieee.to_le_bytes())
                .await?;
            let now = Ieee(self.read_u64(param::MAC_ADDRESS).await?);
            if now != b.coordinator_ieee {
                return Err(HostError::RestoreRefused(format!(
                    "this coordinator kept its IEEE address {now}; the backup needs {}",
                    b.coordinator_ieee
                )));
            }
            self.write(
                param::NWK_FRAME_COUNTER,
                &b.restore_frame_counter().to_le_bytes(),
            )
            .await?;
            self.write(param::NWK_UPDATE_ID, &[b.nwk_update_id]).await?;
        }
        let ieee = Ieee(self.read_u64(param::MAC_ADDRESS).await?);
        self.write(param::APS_DESIGNED_COORDINATOR, &[1]).await?;
        self.write(param::NWK_PANID, &settings.pan_id.to_le_bytes())
            .await?;
        self.write(
            param::APS_EXTENDED_PANID,
            &settings.extended_pan_id.to_le_bytes(),
        )
        .await?;
        self.write(param::USE_PREDEFINED_NWK_PANID, &[1]).await?;
        self.write(
            param::CHANNEL_MASK,
            &(1u32 << settings.channel).to_le_bytes(),
        )
        .await?;
        let mut key = vec![0];
        key.extend_from_slice(&settings.network_key.0);
        self.write(param::NETWORK_KEY, &key).await?;
        self.write(param::SECURITY_MODE, &[security_mode::TC_LINK_KEY])
            .await?;
        self.write(param::TRUST_CENTER_ADDRESS, &ieee.to_le_bytes())
            .await?;
        let mut tclk = ieee.to_le_bytes().to_vec();
        tclk.extend_from_slice(&Key::ZIGBEE_ALLIANCE_09.0);
        self.write(param::LINK_KEY, &tclk).await?;
        self.set_net_state(net_state::CONNECTED).await
    }

    async fn read_network_info(&self) -> Result<NetworkInfo, HostError> {
        Ok(NetworkInfo {
            ieee: Ieee(self.read_u64(param::MAC_ADDRESS).await?),
            nwk: self.read_u16(param::NWK_ADDRESS).await?,
            channel: self
                .read(param::CURRENT_CHANNEL)
                .await?
                .first()
                .copied()
                .unwrap_or(0),
            pan_id: self.read_u16(param::NWK_PANID).await?,
            extended_pan_id: ExtPanId(self.read_u64(param::NWK_EXTENDED_PANID).await?),
            nwk_update_id: self
                .read(param::NWK_UPDATE_ID)
                .await?
                .first()
                .copied()
                .unwrap_or(0),
        })
    }

    async fn send(&self, request: ApsRequest) -> Result<(), HostError> {
        let request = Box::new(request);
        for attempt in 0..5u64 {
            match self
                .deconz
                .request(Request::ApsDataRequest(request.clone()))
                .await
            {
                Ok(_) => return Ok(()),
                Err(DeconzError::Status {
                    status: status::BUSY,
                    ..
                }) => tokio::time::sleep(Duration::from_millis(50 * (attempt + 1))).await,
                Err(e) => return Err(e.into()),
            }
        }
        Err(HostError::Ncp("the coordinator stayed busy".into()))
    }

    fn next_tag(&self) -> u8 {
        self.tag.fetch_add(1, Ordering::Relaxed)
    }
}

impl Adapter for DeconzAdapter {
    fn info(&self) -> AdapterInfo {
        AdapterInfo {
            kind: "deconz",
            firmware: self.deconz.version().text(),
            protocol: (self.protocol & 0xFF) as u8,
            reports_key_exchange: false,
        }
    }

    async fn start(&self, mode: StartMode) -> Result<(StartOutcome, NetworkInfo), HostError> {
        let state = self.device_state().await?;
        self.shared
            .net
            .send_replace(state & device_state::NETWORK_MASK);
        match mode {
            StartMode::Restore(backup) => {
                let settings = NetworkSettings {
                    channel: backup.channel,
                    pan_id: backup.pan_id,
                    extended_pan_id: backup.extended_pan_id,
                    network_key: backup.network_key.key,
                    tx_power: None,
                };
                self.form(&settings, Some(&backup)).await?;
                Ok((StartOutcome::Restored, self.read_network_info().await?))
            }
            StartMode::Normal {
                settings,
                replace_other,
            } => {
                let designed =
                    self.read(param::APS_DESIGNED_COORDINATOR).await?.first() == Some(&1);
                if designed && self.holds(&settings).await? {
                    if state & device_state::NETWORK_MASK != net_state::CONNECTED {
                        self.set_net_state(net_state::CONNECTED).await?;
                    }
                    return Ok((StartOutcome::Resumed, self.read_network_info().await?));
                }
                if designed {
                    let pan = self.read_u16(param::NWK_PANID).await?;
                    let found = format!("PAN {pan:04x}");
                    if !replace_other {
                        return Err(HostError::NetworkMismatch { found });
                    }
                    tracing::warn!(%found, "replacing the coordinator's other network, as configured");
                }
                if settings.tx_power.is_some() {
                    tracing::warn!("deCONZ firmware sets its own transmit power");
                }
                self.form(&settings, None).await?;
                Ok((StartOutcome::Formed, self.read_network_info().await?))
            }
        }
    }

    async fn permit_join(&self, seconds: u8, join: &JoinPolicy) -> Result<(), HostError> {
        if seconds > 0 {
            if join.install_code_only {
                return Err(HostError::Invalid(
                    "deCONZ firmware cannot restrict joining to install codes".into(),
                ));
            }
            if !join.allow_legacy && !self.legacy_warned.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    "deCONZ firmware cannot turn away devices that skip the link key exchange"
                );
            }
        }
        self.write(param::PERMIT_JOIN, &[seconds]).await
    }

    async fn add_join_key(&self, ieee: Ieee, key: Key) -> Result<(), HostError> {
        let mut v = ieee.to_le_bytes().to_vec();
        v.extend_from_slice(&key.0);
        self.write(param::LINK_KEY, &v).await
    }

    async fn send_unicast(
        &self,
        nwk: NodeId,
        aps: Aps,
        payload: Vec<u8>,
    ) -> Result<u16, HostError> {
        let tag = self.next_tag();
        self.send(ApsRequest {
            request_id: tag,
            destination: Address::Nwk(nwk),
            destination_endpoint: Some(aps.destination_endpoint),
            profile: aps.profile,
            cluster: aps.cluster,
            source_endpoint: aps.source_endpoint,
            asdu: payload,
            tx_options: tx_option::ACK,
            radius: 0,
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
        self.send(ApsRequest {
            request_id: self.next_tag(),
            destination: Address::Nwk(destination),
            destination_endpoint: Some(aps.destination_endpoint),
            profile: aps.profile,
            cluster: aps.cluster,
            source_endpoint: aps.source_endpoint,
            asdu: payload,
            tx_options: 0,
            radius: 0,
        })
        .await
    }

    async fn network_info(&self) -> Result<NetworkInfo, HostError> {
        self.read_network_info().await
    }

    async fn backup(&self) -> Result<Backup, HostError> {
        let info = self.read_network_info().await?;
        let frame_counter = self
            .read(param::NWK_FRAME_COUNTER)
            .await
            .ok()
            .and_then(|v| <[u8; 4]>::try_from(v.as_slice()).ok())
            .map(u32::from_le_bytes)
            .ok_or_else(|| {
                HostError::Ncp("this firmware does not report its frame counter".into())
            })?;
        Ok(Backup {
            metadata: Metadata {
                format: backup::FORMAT.into(),
                version: 1,
                source: concat!("zigbee-bridge@", env!("CARGO_PKG_VERSION")).into(),
                internal: Internal {
                    created_unix: Some(now_unix()),
                    ezsp_version: None,
                    firmware: Some(self.deconz.version().text()),
                },
            },
            stack_specific: None,
            coordinator_ieee: info.ieee,
            pan_id: info.pan_id,
            extended_pan_id: info.extended_pan_id,
            nwk_update_id: info.nwk_update_id,
            security_level: 5,
            channel: info.channel,
            channel_mask: vec![info.channel],
            network_key: NetworkKey {
                key: self.network_key().await?,
                sequence_number: 0,
                frame_counter,
            },
            devices: Vec::new(),
        })
    }

    async fn active_scan(&self, _channel_mask: u32) -> Result<Vec<Beacon>, HostError> {
        // The serial protocol has no network discovery.
        Ok(Vec::new())
    }

    async fn leave(&self) -> Result<(), HostError> {
        self.set_net_state(net_state::OFFLINE).await?;
        self.write(param::APS_DESIGNED_COORDINATOR, &[0]).await?;
        self.write(param::NETWORK_KEY, &[0; 17]).await?;
        self.write(param::NWK_PANID, &0xFFFFu16.to_le_bytes()).await
    }

    async fn energy_scan(&self, channel_mask: u32) -> Result<Vec<(u8, i8)>, HostError> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut s) = self.shared.scan.lock() {
            if s.is_some() {
                return Err(HostError::Invalid("a scan is already running".into()));
            }
            *s = Some(tx);
        }
        let mut asdu = vec![self.next_tag()];
        asdu.extend_from_slice(&channel_mask.to_le_bytes());
        asdu.extend_from_slice(&[3, 1]);
        let sent = self
            .send(ApsRequest {
                request_id: self.next_tag(),
                destination: Address::Nwk(0x0000),
                destination_endpoint: Some(0),
                profile: zigbee_zdo::PROFILE_ID,
                cluster: 0x0038,
                source_endpoint: 0,
                asdu,
                tx_options: 0,
                radius: 0,
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
