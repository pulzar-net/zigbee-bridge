//! The coordinator host.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, oneshot, Semaphore};
use zigbee_types::{Ieee, Key, NodeId, BROADCAST_ROUTERS};
use zigbee_zcl::foundation::{self, Global, ReadRecord, ReportingConfig};
use zigbee_zcl::{Direction, Frame, FrameType, Value};
use zigbee_zdo::{Body, Message, Neighbor};

use crate::adapter::{
    Adapter, AdapterEvent, AdapterInfo, Aps, JoinPolicy, NetworkInfo, StartMode, StartOutcome,
};
use crate::backup::Backup;
use crate::device::{Device, InterviewStage};
use crate::error::HostError;
use crate::install_code;
use crate::store::{self, Dirty};
use crate::topology::{Link, Topology};

mod ota_server;
pub use ota_server::OtaOffer;

/// The host's endpoint, which devices bind to.
pub const HOST_ENDPOINT: u8 = 1;

/// Host settings.
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// Where the device store lives; `None` keeps devices in memory only.
    pub store_path: Option<PathBuf>,
    /// Where the latest backup is written; `None` writes none.
    pub backup_path: Option<PathBuf>,
    /// Trust center join policy.
    pub join: JoinPolicy,
    /// Bind and configure reporting for standard clusters at interview.
    pub configure_reporting: bool,
    /// How long to wait for each device response.
    pub request_timeout: Duration,
    /// Attempts per interview request.
    pub request_attempts: u32,
    /// Periodic backups (also taken after joins and leaves).
    pub backup_interval: Duration,
    /// How long a new device has to obtain a unique link key before it is
    /// removed, when legacy devices are not allowed.
    pub legacy_grace: Duration,
    /// Requests in flight at once, so the NCP's buffers are never exhausted.
    pub max_in_flight: usize,
    /// A directory of OTA upgrade images; `None` serves none.
    pub ota_dir: Option<PathBuf>,
    /// How long to wait for a device's OTA query after notifying it.
    pub ota_query_timeout: Duration,
}

impl Default for HostConfig {
    fn default() -> Self {
        HostConfig {
            store_path: None,
            backup_path: None,
            join: JoinPolicy::default(),
            configure_reporting: true,
            request_timeout: Duration::from_secs(10),
            request_attempts: 3,
            backup_interval: Duration::from_secs(3600),
            legacy_grace: Duration::from_secs(30),
            max_in_flight: 8,
            ota_dir: None,
            ota_query_timeout: Duration::from_secs(60),
        }
    }
}

/// What happened on the network.
#[derive(Debug, Clone)]
pub enum HostEvent {
    /// The network is up.
    Started {
        /// Resumed, formed or restored.
        outcome: StartOutcome,
        /// The network.
        network: NetworkInfo,
        /// The NCP.
        adapter: AdapterInfo,
    },
    /// Joining opened (seconds > 0) or closed.
    PermitJoin {
        /// Seconds open, 0 when closed.
        seconds: u8,
        /// The router it was opened on, if not everywhere.
        via: Option<Ieee>,
    },
    /// A device joined or rejoined.
    DeviceJoined {
        /// The device.
        ieee: Ieee,
        /// Its network address.
        nwk: NodeId,
        /// A rejoin.
        rejoin: bool,
    },
    /// A device announced itself (after joining or a power cycle).
    DeviceAnnounced {
        /// The device.
        ieee: Ieee,
        /// Its network address.
        nwk: NodeId,
    },
    /// A device asked for an OTA image (on its own, or after
    /// [`Host::ota_check`] or [`Host::ota_update`]).
    OtaQuery {
        /// The device.
        ieee: Ieee,
        /// The file version it runs.
        current_version: u32,
        /// The newest newer version in the image directory.
        available: Option<u32>,
    },
    /// An OTA download advanced (every 10%).
    OtaProgress {
        /// The device.
        ieee: Ieee,
        /// Percent downloaded.
        percent: u8,
    },
    /// An OTA upgrade ended.
    OtaFinished {
        /// The device.
        ieee: Ieee,
        /// The version it ran.
        from: u32,
        /// The version it downloaded.
        to: u32,
        /// The device applied it.
        success: bool,
        /// The device's upgrade end status.
        status: u8,
    },
    /// The network moved to another channel.
    ChannelChanged {
        /// The network now.
        network: NetworkInfo,
    },
    /// A device left the network.
    DeviceLeft {
        /// The device.
        ieee: Ieee,
    },
    /// The host removed a device.
    DeviceRemoved {
        /// The device.
        ieee: Ieee,
        /// Why.
        reason: String,
    },
    /// An interview started or resumed.
    InterviewStarted {
        /// The device.
        ieee: Ieee,
    },
    /// An interview step finished.
    InterviewStep {
        /// The device.
        ieee: Ieee,
        /// The step that finished.
        stage: InterviewStage,
    },
    /// An interview finished.
    InterviewDone {
        /// The device as interviewed.
        device: Box<Device>,
    },
    /// An interview stopped on a failure; it resumes when the device next
    /// talks.
    InterviewFailed {
        /// The device.
        ieee: Ieee,
        /// The step that failed.
        stage: InterviewStage,
        /// Why.
        error: String,
    },
    /// Attribute values arrived (a report, or an unsolicited read response).
    Attributes {
        /// The device.
        ieee: Ieee,
        /// Its endpoint.
        endpoint: u8,
        /// The cluster.
        cluster: u16,
        /// Attribute ids and values.
        attributes: Vec<(u16, Value)>,
        /// Last hop link quality.
        lqi: u8,
    },
    /// A device sent a cluster command (a remote's button, an alarm).
    Command {
        /// The device.
        ieee: Ieee,
        /// Its endpoint.
        endpoint: u8,
        /// The cluster.
        cluster: u16,
        /// Command id.
        command: u8,
        /// Its name, when the cluster library knows it.
        name: Option<&'static str>,
        /// Decoded fields, when known.
        fields: Vec<(&'static str, Value)>,
        /// Sent by the cluster's server (true) or a client.
        from_server: bool,
    },
    /// A ZCL frame from a device, exactly as received, with how it arrived.
    /// Sent for every frame that is not a reply to the host's own request,
    /// before the decoded [`HostEvent::Attributes`] or
    /// [`HostEvent::Command`], so a device library can decode what the
    /// host does not.
    Frame {
        /// The device.
        ieee: Ieee,
        /// How it arrived.
        link: FrameLink,
        /// The ZCL frame: header and payload.
        frame: Vec<u8>,
    },
    /// A backup was written.
    BackupWritten {
        /// The file.
        path: PathBuf,
    },
    /// The NCP failed; the host has stopped. Restart it.
    AdapterFailed {
        /// Why.
        reason: String,
    },
}

/// How a frame arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLink {
    /// The sender's network address.
    pub nwk: NodeId,
    /// The sender's endpoint.
    pub source_endpoint: u8,
    /// The endpoint it was sent to.
    pub destination_endpoint: u8,
    /// Application profile.
    pub profile: u16,
    /// Cluster.
    pub cluster: u16,
    /// The group, for a multicast.
    pub group: Option<u16>,
    /// Sent as a broadcast or multicast.
    pub broadcast: bool,
    /// Last hop link quality.
    pub lqi: u8,
    /// Last hop signal strength, dBm, when the coordinator reports it
    /// (EmberZNet and deCONZ do; Z-Stack does not).
    pub rssi: Option<i8>,
    /// The device the frame came from on its last hop: the last relay of
    /// the sender's recorded route, or the sender itself.
    pub last_hop: NodeId,
}

/// A ZCL frame for [`Host::zcl_raw`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawZcl {
    /// The device's endpoint.
    pub endpoint: u8,
    /// Cluster.
    pub cluster: u16,
    /// The ZCL frame (header and payload); its sequence number is replaced.
    pub frame: Vec<u8>,
    /// Application profile; the endpoint's own when `None`.
    pub profile: Option<u16>,
    /// The host's endpoint to send from.
    pub source_endpoint: u8,
    /// Wait for the device's reply.
    pub response: bool,
    /// How long to wait; the host's request timeout when `None`.
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum WaitKey {
    Zdo { nwk: NodeId, cluster: u16, tsn: u8 },
    Zcl { nwk: NodeId, cluster: u16, seq: u8 },
}

/// A reply, decoded and as received.
enum Reply {
    Zdo(Message, Vec<u8>),
    Zcl(Frame, Vec<u8>),
}

#[derive(Default)]
struct Waiters {
    pending: HashMap<WaitKey, oneshot::Sender<Result<Reply, HostError>>>,
    tags: HashMap<u16, WaitKey>,
    /// Sends that wait only for APS delivery, by tag.
    deliveries: HashMap<u16, oneshot::Sender<bool>>,
    /// The latest delivery reports, for a report that arrives before its
    /// sender has registered the tag.
    recent: std::collections::VecDeque<(u16, bool)>,
}

/// How many delivery reports are remembered.
const RECENT_DELIVERIES: usize = 64;

impl Waiters {
    fn recent_delivery(&self, tag: u16) -> Option<bool> {
        self.recent
            .iter()
            .rev()
            .find(|(t, _)| *t == tag)
            .map(|(_, d)| *d)
    }
}

#[derive(Default)]
struct State {
    devices: BTreeMap<Ieee, Device>,
    by_nwk: HashMap<NodeId, Ieee>,
    install_codes: HashMap<Ieee, Key>,
    permit_generation: u64,
    legacy_deadlines: HashMap<Ieee, Instant>,
    topology: Topology,
    interviewing: HashSet<Ieee>,
    last_interview: HashMap<Ieee, Instant>,
    last_scan: HashMap<Ieee, Instant>,
    removing: HashSet<Ieee>,
    ota: HashMap<Ieee, ota_server::OtaSession>,
    backup_wanted: bool,
    last_backup: Option<Instant>,
}

impl State {
    fn put_device(&mut self, device: Device) {
        self.by_nwk.retain(|_, ieee| *ieee != device.ieee);
        self.by_nwk.insert(device.nwk, device.ieee);
        self.devices.insert(device.ieee, device);
    }
}

struct Inner<A: Adapter> {
    adapter: A,
    config: HostConfig,
    network: Mutex<NetworkInfo>,
    state: Mutex<State>,
    waiters: Mutex<Waiters>,
    seq: AtomicU8,
    events: mpsc::UnboundedSender<HostEvent>,
    dirty: Dirty,
    in_flight: Semaphore,
    stopped: AtomicBool,
    shutdown: tokio::sync::watch::Sender<bool>,
    backup_lock: tokio::sync::Mutex<()>,
}

/// A running coordinator host. Clones share it.
pub struct Host<A: Adapter> {
    inner: Arc<Inner<A>>,
}

impl<A: Adapter> Clone for Host<A> {
    fn clone(&self) -> Self {
        Host {
            inner: self.inner.clone(),
        }
    }
}

impl<A: Adapter> std::fmt::Debug for Host<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("network", &*lock(&self.inner.network))
            .finish_non_exhaustive()
    }
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Seconds since 2000-01-01, the ZCL time epoch.
fn zcl_time_now() -> u32 {
    now_unix().saturating_sub(946_684_800) as u32
}

impl<A: Adapter> Host<A> {
    /// Start the network on `adapter` and run the host. Events arrive on the
    /// returned receiver until the adapter fails.
    pub async fn start(
        adapter: A,
        adapter_events: mpsc::UnboundedReceiver<AdapterEvent>,
        mode: StartMode,
        config: HostConfig,
    ) -> Result<(Host<A>, mpsc::UnboundedReceiver<HostEvent>), HostError> {
        let devices = match &config.store_path {
            Some(path) => store::load(path).map_err(|e| HostError::Io(e.to_string()))?,
            None => BTreeMap::new(),
        };
        let (outcome, network) = adapter.start(mode).await?;
        tracing::info!(?outcome, ieee = %network.ieee, channel = network.channel, pan = format!("{:04x}", network.pan_id), "network up");
        let info = adapter.info();
        let (events, rx) = mpsc::unbounded_channel();
        let mut state = State::default();
        for (_, device) in devices {
            state.put_device(device);
        }
        state.backup_wanted = outcome != StartOutcome::Resumed;
        let max_in_flight = config.max_in_flight.max(1);
        let host = Host {
            inner: Arc::new(Inner {
                adapter,
                config,
                network: Mutex::new(network.clone()),
                state: Mutex::new(state),
                waiters: Mutex::new(Waiters::default()),
                seq: AtomicU8::new(1),
                events,
                dirty: Dirty::default(),
                in_flight: Semaphore::new(max_in_flight),
                stopped: AtomicBool::new(false),
                shutdown: tokio::sync::watch::channel(false).0,
                backup_lock: tokio::sync::Mutex::new(()),
            }),
        };
        host.emit(HostEvent::Started {
            outcome,
            network,
            adapter: info,
        });
        tokio::spawn(host.clone().event_loop(adapter_events));
        tokio::spawn(host.clone().store_writer());
        tokio::spawn(host.clone().ticker());
        Ok((host, rx))
    }

    fn emit(&self, event: HostEvent) {
        let _ = self.inner.events.send(event);
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.inner.state)
    }

    fn next_seq(&self) -> u8 {
        self.inner.seq.fetch_add(1, Ordering::Relaxed)
    }

    /// The network.
    pub fn network(&self) -> NetworkInfo {
        lock(&self.inner.network).clone()
    }

    /// The NCP.
    pub fn adapter_info(&self) -> AdapterInfo {
        self.inner.adapter.info()
    }

    /// The adapter, for NCP-specific calls.
    pub fn adapter(&self) -> &A {
        &self.inner.adapter
    }

    /// Every device.
    pub fn devices(&self) -> Vec<Device> {
        self.state().devices.values().cloned().collect()
    }

    /// One device.
    pub fn device(&self, ieee: Ieee) -> Option<Device> {
        self.state().devices.get(&ieee).cloned()
    }

    /// The device with a network address.
    pub fn device_by_nwk(&self, nwk: NodeId) -> Option<Device> {
        let state = self.state();
        state
            .by_nwk
            .get(&nwk)
            .and_then(|ieee| state.devices.get(ieee))
            .cloned()
    }

    pub(crate) fn update_device(&self, ieee: Ieee, f: impl FnOnce(&mut Device)) -> Option<Device> {
        let updated = {
            let mut state = self.state();
            let device = state.devices.get_mut(&ieee)?;
            f(device);
            device.clone()
        };
        self.inner.dirty.mark();
        Some(updated)
    }

    /// Give a device a name.
    pub fn set_name(&self, ieee: Ieee, name: Option<String>) -> Result<(), HostError> {
        self.update_device(ieee, |d| d.name = name)
            .map(|_| ())
            .ok_or(HostError::UnknownDevice(ieee))
    }

    fn nwk_of(&self, ieee: Ieee) -> Result<NodeId, HostError> {
        if ieee == self.network().ieee {
            return Ok(self.network().nwk);
        }
        self.state()
            .devices
            .get(&ieee)
            .map(|d| d.nwk)
            .ok_or(HostError::UnknownDevice(ieee))
    }

    // ---- requests -------------------------------------------------------

    async fn request(
        &self,
        key: WaitKey,
        nwk: NodeId,
        aps: Aps,
        payload: Vec<u8>,
        timeout: Duration,
    ) -> Result<Reply, HostError> {
        if self.inner.stopped.load(Ordering::SeqCst) {
            return Err(HostError::Stopped);
        }
        let _permit = self
            .inner
            .in_flight
            .acquire()
            .await
            .map_err(|_| HostError::Stopped)?;
        let (tx, rx) = oneshot::channel();
        lock(&self.inner.waiters).pending.insert(key, tx);
        let tag = match self.inner.adapter.send_unicast(nwk, aps, payload).await {
            Ok(tag) => tag,
            Err(e) => {
                lock(&self.inner.waiters).pending.remove(&key);
                return Err(e);
            }
        };
        {
            let mut w = lock(&self.inner.waiters);
            if w.recent_delivery(tag) == Some(false) {
                w.pending.remove(&key);
                return Err(HostError::DeliveryFailed(nwk));
            }
            w.tags.insert(tag, key);
        }
        let result = tokio::time::timeout(timeout, rx).await;
        {
            let mut w = lock(&self.inner.waiters);
            w.pending.remove(&key);
            w.tags.remove(&tag);
        }
        match result {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err(HostError::Stopped),
            Err(_) => Err(HostError::Timeout(nwk)),
        }
    }

    fn timeout_for(&self, nwk: NodeId) -> Duration {
        let sleepy = self.device_by_nwk(nwk).is_some_and(|d| d.is_sleepy());
        if sleepy {
            self.inner.config.request_timeout * 2
        } else {
            self.inner.config.request_timeout
        }
    }

    /// Send a frame and wait for its APS delivery report.
    async fn send_confirmed(
        &self,
        nwk: NodeId,
        aps: Aps,
        payload: Vec<u8>,
        timeout: Duration,
    ) -> Result<(), HostError> {
        if self.inner.stopped.load(Ordering::SeqCst) {
            return Err(HostError::Stopped);
        }
        let _permit = self
            .inner
            .in_flight
            .acquire()
            .await
            .map_err(|_| HostError::Stopped)?;
        let tag = self.inner.adapter.send_unicast(nwk, aps, payload).await?;
        let rx = {
            let mut w = lock(&self.inner.waiters);
            if let Some(delivered) = w.recent_delivery(tag) {
                return if delivered {
                    Ok(())
                } else {
                    Err(HostError::DeliveryFailed(nwk))
                };
            }
            let (tx, rx) = oneshot::channel();
            w.deliveries.insert(tag, tx);
            rx
        };
        let result = tokio::time::timeout(timeout, rx).await;
        lock(&self.inner.waiters).deliveries.remove(&tag);
        match result {
            Ok(Ok(true)) => Ok(()),
            Ok(Ok(false)) => Err(HostError::DeliveryFailed(nwk)),
            Ok(Err(_)) => Err(HostError::Stopped),
            Err(_) => Err(HostError::Timeout(nwk)),
        }
    }

    /// Send a ZCL frame given as bytes, for a caller that builds frames
    /// itself (a device library). The frame's transaction sequence number
    /// is overwritten with the host's own. With `response`, waits for the
    /// device's reply with that sequence number and returns it exactly as
    /// received; otherwise waits for APS delivery and returns `None`.
    pub async fn zcl_raw(&self, ieee: Ieee, request: RawZcl) -> Result<Option<Vec<u8>>, HostError> {
        let nwk = self.nwk_of(ieee)?;
        let mut frame = request.frame;
        let tsn_at = match frame.first() {
            Some(control) if control & 0x04 != 0 => 3,
            Some(_) => 1,
            None => return Err(HostError::Invalid("empty ZCL frame".into())),
        };
        if frame.len() < tsn_at + 2 {
            return Err(HostError::Invalid(
                "ZCL frame shorter than its header".into(),
            ));
        }
        let seq = self.next_seq();
        if let Some(b) = frame.get_mut(tsn_at) {
            *b = seq;
        }
        let profile = request.profile.unwrap_or_else(|| {
            self.device(ieee)
                .map(|d| d.profile_of(request.endpoint))
                .unwrap_or(zigbee_zcl::PROFILE_HOME_AUTOMATION)
        });
        let aps = Aps {
            profile,
            cluster: request.cluster,
            source_endpoint: request.source_endpoint,
            destination_endpoint: request.endpoint,
        };
        let timeout = request.timeout.unwrap_or_else(|| self.timeout_for(nwk));
        if !request.response {
            return self
                .send_confirmed(nwk, aps, frame, timeout)
                .await
                .map(|_| None);
        }
        let key = WaitKey::Zcl {
            nwk,
            cluster: request.cluster,
            seq,
        };
        match self.request(key, nwk, aps, frame, timeout).await? {
            Reply::Zcl(_, raw) => Ok(Some(raw)),
            Reply::Zdo(..) => Err(HostError::BadFrame("ZDO reply to a ZCL request".into())),
        }
    }

    /// Send a ZDO request given as bytes (transaction sequence number
    /// first, then the request). The sequence number is overwritten with
    /// the host's own. With `response`, returns the response payload
    /// exactly as received (sequence number first); otherwise waits for
    /// APS delivery and returns `None`.
    pub async fn zdo_raw(
        &self,
        ieee: Ieee,
        cluster: u16,
        mut payload: Vec<u8>,
        response: bool,
        timeout: Option<Duration>,
    ) -> Result<Option<Vec<u8>>, HostError> {
        if cluster & 0x8000 != 0 {
            return Err(HostError::Invalid(format!(
                "{cluster:#06x} is a ZDO response, not a request"
            )));
        }
        let nwk = self.nwk_of(ieee)?;
        let tsn = self.next_seq();
        match payload.first_mut() {
            Some(b) => *b = tsn,
            None => return Err(HostError::Invalid("empty ZDO payload".into())),
        }
        let aps = Aps {
            profile: zigbee_zdo::PROFILE_ID,
            cluster,
            source_endpoint: 0,
            destination_endpoint: 0,
        };
        let timeout = timeout.unwrap_or_else(|| self.timeout_for(nwk));
        if !response {
            return self
                .send_confirmed(nwk, aps, payload, timeout)
                .await
                .map(|_| None);
        }
        let key = WaitKey::Zdo {
            nwk,
            cluster: cluster | 0x8000,
            tsn,
        };
        match self.request(key, nwk, aps, payload, timeout).await? {
            Reply::Zdo(_, raw) => Ok(Some(raw)),
            Reply::Zcl(..) => Err(HostError::BadFrame("ZCL reply to a ZDO request".into())),
        }
    }

    /// Send a ZDO request to `nwk` and wait for its response.
    pub async fn zdo_request(&self, nwk: NodeId, body: Body) -> Result<Message, HostError> {
        let tsn = self.next_seq();
        let (cluster, payload) = Message { tsn, body }.encode();
        let key = WaitKey::Zdo {
            nwk,
            cluster: cluster | 0x8000,
            tsn,
        };
        let aps = Aps {
            profile: zigbee_zdo::PROFILE_ID,
            cluster,
            source_endpoint: 0,
            destination_endpoint: 0,
        };
        match self
            .request(key, nwk, aps, payload, self.timeout_for(nwk))
            .await?
        {
            Reply::Zdo(m, _) => Ok(m),
            Reply::Zcl(..) => Err(HostError::BadFrame("ZCL reply to a ZDO request".into())),
        }
    }

    /// [`Host::zdo_request`] with the configured retries.
    pub async fn zdo_request_retry(&self, nwk: NodeId, body: Body) -> Result<Message, HostError> {
        let mut last = HostError::Timeout(nwk);
        for attempt in 0..self.inner.config.request_attempts.max(1) {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            match self.zdo_request(nwk, body.clone()).await {
                Ok(m) => return Ok(m),
                Err(
                    e @ (HostError::Timeout(_) | HostError::DeliveryFailed(_) | HostError::Ncp(_)),
                ) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    /// Send a ZCL frame to a device's endpoint and wait for the reply with
    /// the same sequence number. The frame's sequence number is assigned
    /// here.
    pub async fn zcl_request(
        &self,
        ieee: Ieee,
        endpoint: u8,
        cluster: u16,
        mut frame: Frame,
    ) -> Result<Frame, HostError> {
        let nwk = self.nwk_of(ieee)?;
        let profile = self
            .device(ieee)
            .map(|d| d.profile_of(endpoint))
            .unwrap_or(zigbee_zcl::PROFILE_HOME_AUTOMATION);
        frame.header.seq = self.next_seq();
        let key = WaitKey::Zcl {
            nwk,
            cluster,
            seq: frame.header.seq,
        };
        let aps = Aps {
            profile,
            cluster,
            source_endpoint: HOST_ENDPOINT,
            destination_endpoint: endpoint,
        };
        match self
            .request(key, nwk, aps, frame.encode(), self.timeout_for(nwk))
            .await?
        {
            Reply::Zcl(f, _) => Ok(f),
            Reply::Zdo(..) => Err(HostError::BadFrame("ZDO reply to a ZCL request".into())),
        }
    }

    async fn zcl_request_retry(
        &self,
        ieee: Ieee,
        endpoint: u8,
        cluster: u16,
        frame: Frame,
    ) -> Result<Frame, HostError> {
        let mut last = HostError::UnknownDevice(ieee);
        for attempt in 0..self.inner.config.request_attempts.max(1) {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            match self
                .zcl_request(ieee, endpoint, cluster, frame.clone())
                .await
            {
                Ok(f) => return Ok(f),
                Err(
                    e @ (HostError::Timeout(_) | HostError::DeliveryFailed(_) | HostError::Ncp(_)),
                ) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    fn global_reply(frame: &Frame, what: &str) -> Result<Global, HostError> {
        let g = Global::decode(frame.header.command, &frame.payload)
            .map_err(|e| HostError::BadFrame(e.to_string()))?;
        if let Global::DefaultResponse { status, .. } = g {
            if status != zigbee_zcl::status::SUCCESS {
                return Err(HostError::Status {
                    what: what.into(),
                    status,
                });
            }
        }
        Ok(g)
    }

    /// Read attributes. Returns one record per attribute the device answered.
    pub async fn read_attributes(
        &self,
        ieee: Ieee,
        endpoint: u8,
        cluster: u16,
        attributes: &[u16],
    ) -> Result<Vec<ReadRecord>, HostError> {
        let frame = Frame::global(
            Direction::ClientToServer,
            0,
            foundation::id::READ_ATTRIBUTES,
            Global::ReadAttributes(attributes.to_vec()).encode(),
        );
        let reply = self
            .zcl_request_retry(ieee, endpoint, cluster, frame)
            .await?;
        match Self::global_reply(&reply, "read attributes")? {
            Global::ReadAttributesResponse(records) => Ok(records),
            other => Err(HostError::BadFrame(format!("unexpected reply {other:?}"))),
        }
    }

    /// Write attributes. Returns (status, attribute) records; one success
    /// record means every write succeeded.
    pub async fn write_attributes(
        &self,
        ieee: Ieee,
        endpoint: u8,
        cluster: u16,
        values: Vec<(u16, Value)>,
    ) -> Result<Vec<(u8, Option<u16>)>, HostError> {
        let frame = Frame::global(
            Direction::ClientToServer,
            0,
            foundation::id::WRITE_ATTRIBUTES,
            Global::WriteAttributes(values).encode(),
        );
        let reply = self
            .zcl_request_retry(ieee, endpoint, cluster, frame)
            .await?;
        match Self::global_reply(&reply, "write attributes")? {
            Global::WriteAttributesResponse(records) => Ok(records),
            other => Err(HostError::BadFrame(format!("unexpected reply {other:?}"))),
        }
    }

    /// Configure attribute reporting.
    pub async fn configure_reporting(
        &self,
        ieee: Ieee,
        endpoint: u8,
        cluster: u16,
        configs: Vec<ReportingConfig>,
    ) -> Result<Vec<foundation::ReportingStatus>, HostError> {
        let frame = Frame::global(
            Direction::ClientToServer,
            0,
            foundation::id::CONFIGURE_REPORTING,
            Global::ConfigureReporting(configs).encode(),
        );
        let reply = self
            .zcl_request_retry(ieee, endpoint, cluster, frame)
            .await?;
        match Self::global_reply(&reply, "configure reporting")? {
            Global::ConfigureReportingResponse(records) => Ok(records),
            other => Err(HostError::BadFrame(format!("unexpected reply {other:?}"))),
        }
    }

    /// Send a cluster command by name, with field values in order (trailing
    /// optional fields may be left out). Returns the device's reply: its
    /// default response or the command's own response.
    pub async fn command(
        &self,
        ieee: Ieee,
        endpoint: u8,
        cluster: u16,
        name: &str,
        values: &[Value],
    ) -> Result<Frame, HostError> {
        let def = zigbee_zcl::cluster(cluster)
            .and_then(|c| c.command_by_name(Direction::ClientToServer, name))
            .ok_or_else(|| {
                HostError::Invalid(format!("cluster {cluster:#06x} has no command {name:?}"))
            })?;
        let payload = def
            .encode(values)
            .map_err(|e| HostError::Invalid(e.to_string()))?;
        let reply = self
            .zcl_request(
                ieee,
                endpoint,
                cluster,
                Frame::cluster(Direction::ClientToServer, 0, def.id, payload),
            )
            .await?;
        if reply.header.frame_type == FrameType::Global {
            Self::global_reply(&reply, name)?;
        }
        Ok(reply)
    }

    /// Bind a device's cluster to the host's endpoint, so its reports come here.
    pub async fn bind_to_host(
        &self,
        ieee: Ieee,
        endpoint: u8,
        cluster: u16,
    ) -> Result<(), HostError> {
        let nwk = self.nwk_of(ieee)?;
        let body = Body::BindReq(zigbee_zdo::Binding {
            source: ieee,
            source_endpoint: endpoint,
            cluster,
            target: zigbee_zdo::BindTarget::Endpoint {
                ieee: self.network().ieee,
                endpoint: HOST_ENDPOINT,
            },
        });
        let m = self.zdo_request_retry(nwk, body).await?;
        match m.status() {
            Some(zigbee_zdo::status::SUCCESS) => Ok(()),
            Some(status) => Err(HostError::Status {
                what: "bind".into(),
                status,
            }),
            None => Err(HostError::BadFrame("bind response without status".into())),
        }
    }

    // ---- joining --------------------------------------------------------

    /// Open the network for joining for `seconds` (at most 254; 0 closes
    /// it), everywhere or only through one router. Opening through a router
    /// leaves the coordinator's own radio closed.
    pub async fn permit_join(&self, seconds: u8, via: Option<Ieee>) -> Result<(), HostError> {
        let seconds = seconds.min(254);
        let join = self.inner.config.join.clone();
        let local = via.is_none() || seconds == 0;
        self.inner
            .adapter
            .permit_join(if local { seconds } else { 0 }, &join)
            .await?;
        if !local && seconds > 0 {
            // The trust center must accept joins even with its radio closed.
            self.inner.adapter.permit_join(0, &join).await?;
        }
        if seconds > 0 {
            let codes: Vec<(Ieee, Key)> = self
                .state()
                .install_codes
                .iter()
                .map(|(i, k)| (*i, *k))
                .collect();
            for (ieee, key) in codes {
                self.inner.adapter.add_join_key(ieee, key).await?;
            }
        }
        let tsn = self.next_seq();
        let (cluster, payload) = Message {
            tsn,
            body: Body::MgmtPermitJoiningReq {
                duration: seconds,
                tc_significance: 1,
            },
        }
        .encode();
        let aps = Aps {
            profile: zigbee_zdo::PROFILE_ID,
            cluster,
            source_endpoint: 0,
            destination_endpoint: 0,
        };
        match via {
            Some(router) => {
                let nwk = self.nwk_of(router)?;
                self.inner.adapter.send_unicast(nwk, aps, payload).await?;
            }
            None => {
                self.inner
                    .adapter
                    .send_broadcast(BROADCAST_ROUTERS, aps, payload)
                    .await?
            }
        }
        let generation = {
            let mut state = self.state();
            state.permit_generation += 1;
            state.permit_generation
        };
        self.emit(HostEvent::PermitJoin { seconds, via });
        if seconds > 0 {
            let host = self.clone();
            tokio::spawn(async move {
                let mut shutdown = host.inner.shutdown.subscribe();
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(u64::from(seconds))) => {}
                    _ = shutdown.wait_for(|stop| *stop) => return,
                }
                if host.state().permit_generation == generation {
                    // The NCP closed itself; restore the rejoin-only policy.
                    if let Err(e) = host
                        .inner
                        .adapter
                        .permit_join(0, &host.inner.config.join)
                        .await
                    {
                        tracing::warn!(error = %e, "closing the network");
                    }
                    host.emit(HostEvent::PermitJoin {
                        seconds: 0,
                        via: None,
                    });
                }
            });
        }
        Ok(())
    }

    /// Accept a device's install code (with its CRC). The derived key is
    /// given to the NCP now and whenever joining opens, until the device
    /// joins. Install codes are kept in memory only.
    pub async fn add_install_code(
        &self,
        ieee: Ieee,
        code_with_crc: &[u8],
    ) -> Result<(), HostError> {
        let key =
            install_code::link_key(code_with_crc).map_err(|e| HostError::Invalid(e.to_string()))?;
        self.state().install_codes.insert(ieee, key);
        self.inner.adapter.add_join_key(ieee, key).await
    }

    /// Ask a device to leave and forget it. With `force`, it is forgotten
    /// even if it does not answer (a dead or sleepy device).
    pub async fn remove(&self, ieee: Ieee, force: bool) -> Result<(), HostError> {
        self.remove_because(ieee, force, "removed").await
    }

    async fn remove_because(&self, ieee: Ieee, force: bool, reason: &str) -> Result<(), HostError> {
        let nwk = self.nwk_of(ieee)?;
        self.state().removing.insert(ieee);
        let result = self
            .zdo_request(nwk, Body::MgmtLeaveReq { ieee, flags: 0 })
            .await;
        if let Err(e) = &result {
            if !force {
                self.state().removing.remove(&ieee);
                return Err(e.clone());
            }
            tracing::warn!(%ieee, error = %e, "device did not acknowledge leave; forgetting it");
        }
        self.forget(ieee, None);
        self.state().removing.remove(&ieee);
        self.emit(HostEvent::DeviceRemoved {
            ieee,
            reason: reason.into(),
        });
        Ok(())
    }

    /// Drop a device's record; `reason` is reported as a removal, `None`
    /// for a device that left by itself.
    fn forget(&self, ieee: Ieee, reason: Option<&str>) {
        let removed = {
            let mut state = self.state();
            let removed = state.devices.remove(&ieee);
            if let Some(d) = &removed {
                state.by_nwk.remove(&d.nwk);
                state.topology.remove_node(d.nwk);
            }
            state.legacy_deadlines.remove(&ieee);
            state.interviewing.remove(&ieee);
            state.backup_wanted = true;
            removed.is_some()
        };
        if removed {
            self.inner.dirty.mark();
            if let Some(reason) = reason {
                self.emit(HostEvent::DeviceRemoved {
                    ieee,
                    reason: reason.into(),
                });
            }
        }
    }

    // ---- topology -------------------------------------------------------

    /// Current links, with addresses resolved to IEEE where known.
    pub fn topology(&self) -> Vec<(Option<Ieee>, Option<Ieee>, Link)> {
        let state = self.state();
        let coordinator = (self.network().nwk, self.network().ieee);
        let resolve = |nwk: NodeId| {
            if nwk == coordinator.0 {
                Some(coordinator.1)
            } else {
                state.by_nwk.get(&nwk).copied()
            }
        };
        state
            .topology
            .links()
            .into_iter()
            .map(|l| (resolve(l.from), resolve(l.to), l))
            .collect()
    }

    /// Read a router's (or the coordinator's) whole neighbor table. At most
    /// once a minute per device.
    pub async fn scan_neighbors(&self, ieee: Ieee) -> Result<Vec<Neighbor>, HostError> {
        {
            let mut state = self.state();
            if state
                .last_scan
                .get(&ieee)
                .is_some_and(|t| t.elapsed() < Duration::from_secs(60))
            {
                return Err(HostError::Invalid(
                    "neighbor scans are limited to one a minute per device".into(),
                ));
            }
            state.last_scan.insert(ieee, Instant::now());
        }
        let nwk = self.nwk_of(ieee)?;
        let mut neighbors = Vec::new();
        let mut start = 0u8;
        loop {
            let m = self
                .zdo_request_retry(nwk, Body::MgmtLqiReq { start_index: start })
                .await?;
            match m.body {
                Body::MgmtLqiRsp {
                    status: zigbee_zdo::status::SUCCESS,
                    total,
                    neighbors: page,
                    ..
                } => {
                    let got = page.len();
                    neighbors.extend(page);
                    if got == 0 || neighbors.len() >= usize::from(total) || neighbors.len() >= 255 {
                        break;
                    }
                    start = neighbors.len() as u8;
                }
                Body::MgmtLqiRsp { status, .. } => {
                    return Err(HostError::Status {
                        what: "Mgmt_Lqi".into(),
                        status,
                    })
                }
                _ => return Err(HostError::BadFrame("unexpected Mgmt_Lqi reply".into())),
            }
        }
        let now = now_unix();
        let mut state = self.state();
        for n in &neighbors {
            state.topology.neighbor(nwk, n.nwk, n.lqi, now);
        }
        Ok(neighbors)
    }

    /// Move the network to `channel` (11 to 26): broadcast a network
    /// update with the next update id and wait until the coordinator runs
    /// on the new channel. Devices follow the broadcast; sleepy ones that
    /// miss it find the network again when they next rejoin.
    pub async fn change_channel(&self, channel: u8) -> Result<NetworkInfo, HostError> {
        if !(11..=26).contains(&channel) {
            return Err(HostError::Invalid(format!(
                "channel {channel} is not 11 to 26"
            )));
        }
        let current = self.network();
        if current.channel == channel {
            return Ok(current);
        }
        let update_id = current.nwk_update_id.wrapping_add(1);
        let (cluster, payload) = Message {
            tsn: self.next_seq(),
            body: Body::MgmtNwkUpdateReq {
                channel_mask: 1 << channel,
                scan_duration: 0xFE,
                scan_count: None,
                nwk_update_id: Some(update_id),
                manager: None,
            },
        }
        .encode();
        let aps = Aps {
            profile: zigbee_zdo::PROFILE_ID,
            cluster,
            source_endpoint: 0,
            destination_endpoint: 0,
        };
        self.inner
            .adapter
            .send_broadcast(zigbee_types::BROADCAST_RX_ON_WHEN_IDLE, aps, payload)
            .await?;
        // Devices switch after the broadcast has spread (nwkNetworkBroadcast
        // DeliveryTime); the coordinator with them.
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let mut now = self.inner.adapter.network_info().await?;
            if now.channel == channel {
                // Z-Stack does not report the update id: keep the one sent.
                if now.nwk_update_id == 0 {
                    now.nwk_update_id = update_id;
                }
                *lock(&self.inner.network) = now.clone();
                self.state().backup_wanted = true;
                self.inner.dirty.mark();
                tracing::info!(
                    channel,
                    update_id = now.nwk_update_id,
                    "network moved to a new channel"
                );
                self.emit(HostEvent::ChannelChanged {
                    network: now.clone(),
                });
                return Ok(now);
            }
            if Instant::now() > deadline {
                return Err(HostError::Ncp(format!(
                    "the coordinator is still on channel {} after asking for {channel}",
                    now.channel
                )));
            }
        }
    }

    /// Measure every channel's energy (channel, max RSSI in dBm).
    pub async fn energy_scan(&self) -> Result<Vec<(u8, i8)>, HostError> {
        self.inner.adapter.energy_scan(0x07FF_F800).await
    }

    // ---- backups and the store -------------------------------------------

    /// Take a backup now, and write it if a backup path is configured.
    pub async fn backup(&self) -> Result<Backup, HostError> {
        let _one_at_a_time = self.inner.backup_lock.lock().await;
        let mut backup = self.inner.adapter.backup().await?;
        {
            let state = self.state();
            let coordinator = self.network().nwk;
            for entry in &mut backup.devices {
                if let Some(d) = state.devices.get(&entry.ieee_address) {
                    entry.nwk_address = Some(d.nwk);
                    entry.is_child = d.parent == Some(coordinator);
                }
            }
            for d in state.devices.values() {
                if !backup.devices.iter().any(|e| e.ieee_address == d.ieee) {
                    backup.devices.push(crate::backup::BackupDevice {
                        nwk_address: Some(d.nwk),
                        ieee_address: d.ieee,
                        is_child: d.parent == Some(coordinator),
                        link_key: None,
                    });
                }
            }
        }
        if let Some(path) = &self.inner.config.backup_path {
            let json = backup.to_json();
            let target = path.clone();
            tokio::task::spawn_blocking(move || store::write_atomic(&target, json.as_bytes()))
                .await
                .map_err(|e| HostError::Io(e.to_string()))?
                .map_err(|e| HostError::Io(format!("writing backup {}: {e}", path.display())))?;
            self.emit(HostEvent::BackupWritten { path: path.clone() });
        }
        let mut state = self.state();
        state.backup_wanted = false;
        state.last_backup = Some(Instant::now());
        Ok(backup)
    }

    /// Stop the host: write the device store, end its tasks and release the
    /// adapter (and with it the port) once every clone is dropped.
    pub async fn shutdown(&self) -> Result<(), HostError> {
        self.stop();
        let _ = self.inner.shutdown.send(true);
        self.flush().await
    }

    /// Write the device store now.
    pub async fn flush(&self) -> Result<(), HostError> {
        let Some(path) = self.inner.config.store_path.clone() else {
            return Ok(());
        };
        let bytes = store::serialize(&self.state().devices);
        tokio::task::spawn_blocking(move || store::write_atomic(&path, &bytes))
            .await
            .map_err(|e| HostError::Io(e.to_string()))?
            .map_err(|e| HostError::Io(e.to_string()))
    }

    async fn store_writer(self) {
        if self.inner.config.store_path.is_none() {
            return;
        }
        loop {
            self.inner.dirty.changed().await;
            tokio::time::sleep(store::WRITE_DELAY).await;
            if let Err(e) = self.flush().await {
                tracing::error!(error = %e, "writing the device store");
            }
            if self.inner.stopped.load(Ordering::SeqCst) {
                return;
            }
        }
    }

    async fn ticker(self) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if self.inner.stopped.load(Ordering::SeqCst) {
                return;
            }
            let now = Instant::now();
            let (expired, backup_due) = {
                let mut state = self.state();
                let expired: Vec<Ieee> = state
                    .legacy_deadlines
                    .iter()
                    .filter(|(_, t)| now >= **t)
                    .map(|(i, _)| *i)
                    .collect();
                for ieee in &expired {
                    state.legacy_deadlines.remove(ieee);
                }
                let interval_due = state
                    .last_backup
                    .is_none_or(|t| t.elapsed() >= self.inner.config.backup_interval);
                let settled = state
                    .last_backup
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(10));
                state.topology.expire(now_unix());
                (
                    expired,
                    self.inner.config.backup_path.is_some()
                        && ((state.backup_wanted && settled) || interval_due),
                )
            };
            for ieee in expired {
                tracing::warn!(%ieee, "device did not request a unique link key; removing it (legacy devices are not allowed)");
                let host = self.clone();
                tokio::spawn(async move {
                    let reason = "no unique link key (pre-Zigbee 3.0 devices are not allowed)";
                    if host.remove_because(ieee, true, reason).await.is_err() {
                        host.forget(ieee, Some(reason));
                    }
                });
            }
            if backup_due {
                if let Err(e) = self.backup().await {
                    tracing::error!(error = %e, "taking a backup");
                    self.state().last_backup = Some(Instant::now());
                }
            }
        }
    }

    // ---- incoming --------------------------------------------------------

    async fn event_loop(self, mut events: mpsc::UnboundedReceiver<AdapterEvent>) {
        let mut shutdown = self.inner.shutdown.subscribe();
        loop {
            let event = tokio::select! {
                event = events.recv() => match event {
                    Some(event) => event,
                    None => break,
                },
                _ = shutdown.wait_for(|stop| *stop) => return,
            };
            match event {
                AdapterEvent::Message {
                    source,
                    source_ieee,
                    aps,
                    broadcast,
                    group,
                    lqi,
                    rssi,
                    payload,
                } => self.on_message(
                    source,
                    source_ieee,
                    aps,
                    broadcast,
                    group,
                    lqi,
                    rssi,
                    payload,
                ),
                AdapterEvent::Joined {
                    nwk,
                    ieee,
                    parent,
                    rejoin,
                } => self.on_joined(nwk, ieee, parent, rejoin),
                AdapterEvent::Left { ieee, .. } => {
                    tracing::info!(%ieee, "device left");
                    let ours = self.state().removing.contains(&ieee);
                    self.forget(ieee, None);
                    if !ours {
                        self.emit(HostEvent::DeviceLeft { ieee });
                    }
                }
                AdapterEvent::KeyExchange { ieee, success } => {
                    if success {
                        self.state().legacy_deadlines.remove(&ieee);
                        self.update_device(ieee, |d| d.unique_link_key = true);
                    }
                }
                AdapterEvent::RouteRecord {
                    source,
                    ieee,
                    relays,
                    lqi,
                    rssi,
                } => {
                    let coordinator = self.network().nwk;
                    let mut state = self.state();
                    state.topology.route_record(
                        source,
                        &relays,
                        coordinator,
                        lqi,
                        rssi,
                        now_unix(),
                    );
                    let known = ieee.filter(|i| state.devices.contains_key(i));
                    if let (false, Some(ieee)) = (state.by_nwk.contains_key(&source), known) {
                        if let Some(mut d) = state.devices.get(&ieee).cloned() {
                            d.nwk = source;
                            state.put_device(d);
                        }
                    }
                }
                AdapterEvent::Delivery { tag, delivered } => {
                    let mut w = lock(&self.inner.waiters);
                    if w.recent.len() >= RECENT_DELIVERIES {
                        w.recent.pop_front();
                    }
                    w.recent.push_back((tag, delivered));
                    if let Some(tx) = w.deliveries.remove(&tag) {
                        let _ = tx.send(delivered);
                    }
                    if let Some(key) = w.tags.remove(&tag) {
                        if !delivered {
                            let nwk = match key {
                                WaitKey::Zdo { nwk, .. } | WaitKey::Zcl { nwk, .. } => nwk,
                            };
                            if let Some(tx) = w.pending.remove(&key) {
                                let _ = tx.send(Err(HostError::DeliveryFailed(nwk)));
                            }
                        }
                    }
                }
                AdapterEvent::NetworkUp => tracing::info!("network up"),
                AdapterEvent::NetworkDown => tracing::warn!("network down"),
                AdapterEvent::Failed(reason) => {
                    tracing::error!(%reason, "adapter failed");
                    self.stop();
                    self.emit(HostEvent::AdapterFailed { reason });
                    return;
                }
            }
        }
        self.stop();
        self.emit(HostEvent::AdapterFailed {
            reason: "adapter events ended".into(),
        });
    }

    fn stop(&self) {
        self.inner.stopped.store(true, Ordering::SeqCst);
        let mut w = lock(&self.inner.waiters);
        for (_, tx) in w.pending.drain() {
            let _ = tx.send(Err(HostError::Stopped));
        }
        self.inner.dirty.mark();
    }

    fn on_joined(&self, nwk: NodeId, ieee: Ieee, parent: NodeId, rejoin: bool) {
        let is_new = {
            let mut state = self.state();
            let mut device = state
                .devices
                .get(&ieee)
                .cloned()
                .unwrap_or_else(|| Device::new(ieee, nwk, now_unix()));
            let is_new = !state.devices.contains_key(&ieee);
            device.nwk = nwk;
            device.parent = Some(parent);
            state.put_device(device);
            state.install_codes.remove(&ieee);
            if is_new
                && !rejoin
                && !self.inner.config.join.allow_legacy
                && self.inner.adapter.info().reports_key_exchange
            {
                state
                    .legacy_deadlines
                    .insert(ieee, Instant::now() + self.inner.config.legacy_grace);
            }
            state.backup_wanted = true;
            is_new
        };
        self.inner.dirty.mark();
        tracing::info!(%ieee, nwk = format!("{nwk:#06x}"), rejoin, "device joined");
        self.emit(HostEvent::DeviceJoined { ieee, nwk, rejoin });
        if is_new {
            // Most devices announce themselves right after joining; start
            // the interview then, or shortly after if no announce comes.
            let host = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                host.maybe_interview(ieee, true);
            });
        }
    }

    fn on_announce(&self, nwk: NodeId, ieee: Ieee) {
        let is_new = {
            let mut state = self.state();
            let is_new = !state.devices.contains_key(&ieee);
            let mut device = state
                .devices
                .get(&ieee)
                .cloned()
                .unwrap_or_else(|| Device::new(ieee, nwk, now_unix()));
            device.nwk = nwk;
            state.put_device(device);
            if is_new {
                state.backup_wanted = true;
            }
            is_new
        };
        self.inner.dirty.mark();
        if is_new {
            // A coordinator that reports no joins (deCONZ): the announce is
            // the first the host hears of the device.
            tracing::info!(%ieee, nwk = format!("{nwk:#06x}"), "device joined (announce)");
            self.emit(HostEvent::DeviceJoined {
                ieee,
                nwk,
                rejoin: false,
            });
        }
        self.emit(HostEvent::DeviceAnnounced { ieee, nwk });
        self.maybe_interview(ieee, true);
    }

    /// Interview a device again from the start (after a firmware update, or
    /// to reconfigure reporting).
    pub fn reinterview(&self, ieee: Ieee) -> Result<(), HostError> {
        self.update_device(ieee, |d| d.interview = crate::device::Interview::default())
            .ok_or(HostError::UnknownDevice(ieee))?;
        self.state().last_interview.remove(&ieee);
        self.maybe_interview(ieee, true);
        Ok(())
    }

    /// Start or resume an interview unless one is running, done, or was
    /// tried too recently.
    fn maybe_interview(&self, ieee: Ieee, eager: bool) {
        {
            let mut state = self.state();
            let Some(device) = state.devices.get(&ieee) else {
                return;
            };
            if device.interview.is_done() || state.interviewing.contains(&ieee) {
                return;
            }
            let backoff = if eager {
                Duration::ZERO
            } else {
                Duration::from_secs(30)
            };
            if state
                .last_interview
                .get(&ieee)
                .is_some_and(|t| t.elapsed() < backoff)
            {
                return;
            }
            state.interviewing.insert(ieee);
            state.last_interview.insert(ieee, Instant::now());
        }
        let host = self.clone();
        tokio::spawn(async move {
            crate::interview::run(&host, ieee).await;
            host.state().interviewing.remove(&ieee);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn on_message(
        &self,
        source: NodeId,
        source_ieee: Option<Ieee>,
        aps: Aps,
        broadcast: bool,
        group: Option<u16>,
        lqi: u8,
        rssi: Option<i8>,
        payload: Vec<u8>,
    ) {
        let now = now_unix();
        let known = {
            let mut state = self.state();
            if let Some(ieee) = source_ieee {
                if state.by_nwk.get(&source) != Some(&ieee) {
                    if let Some(mut d) = state.devices.get(&ieee).cloned() {
                        d.nwk = source;
                        state.put_device(d);
                    }
                }
            }
            let coordinator = self.network().nwk;
            state.topology.last_hop(source, coordinator, lqi, rssi, now);
            let ieee = state.by_nwk.get(&source).copied();
            if let Some(d) = ieee.and_then(|i| state.devices.get_mut(&i)) {
                d.last_seen_unix = Some(now);
                d.last_lqi = Some(lqi);
                d.last_rssi = rssi.or(d.last_rssi);
            }
            ieee
        };

        if aps.profile == zigbee_zdo::PROFILE_ID {
            match Message::parse(aps.cluster, &payload) {
                Ok(m) if aps.cluster & 0x8000 != 0 => self.resolve(
                    WaitKey::Zdo {
                        nwk: source,
                        cluster: aps.cluster,
                        tsn: m.tsn,
                    },
                    Reply::Zdo(m, payload.clone()),
                ),
                Ok(Message {
                    body: Body::DeviceAnnce { nwk, ieee, .. },
                    ..
                }) => self.on_announce(nwk, ieee),
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!(source, cluster = aps.cluster, error = %e, "dropping malformed ZDO frame")
                }
            }
            return;
        }

        let frame = match Frame::parse(&payload) {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(source, cluster = aps.cluster, error = %e, "dropping malformed ZCL frame");
                return;
            }
        };
        let key = WaitKey::Zcl {
            nwk: source,
            cluster: aps.cluster,
            seq: frame.header.seq,
        };
        let is_reply = lock(&self.inner.waiters).pending.contains_key(&key) && !broadcast;
        if is_reply {
            self.resolve(key, Reply::Zcl(frame, payload));
        } else if let Some(ieee) = known {
            let last_hop = self
                .state()
                .topology
                .route(source)
                .and_then(|r| r.last().copied())
                .unwrap_or(source);
            self.emit(HostEvent::Frame {
                ieee,
                link: FrameLink {
                    nwk: source,
                    source_endpoint: aps.source_endpoint,
                    destination_endpoint: aps.destination_endpoint,
                    profile: aps.profile,
                    cluster: aps.cluster,
                    group,
                    broadcast,
                    lqi,
                    rssi,
                    last_hop,
                },
                frame: payload,
            });
            self.on_zcl(ieee, source, aps, broadcast, lqi, frame);
        } else {
            tracing::debug!(
                source = format!("{source:#06x}"),
                cluster = aps.cluster,
                "ZCL frame from an unknown device"
            );
        }
        if let Some(ieee) = known {
            self.maybe_interview(ieee, false);
        }
    }

    fn resolve(&self, key: WaitKey, reply: Reply) {
        if let Some(tx) = lock(&self.inner.waiters).pending.remove(&key) {
            let _ = tx.send(Ok(reply));
        }
    }

    fn on_zcl(&self, ieee: Ieee, source: NodeId, aps: Aps, broadcast: bool, lqi: u8, frame: Frame) {
        let h = frame.header;
        let mut wants_default_response = !h.disable_default_response && !broadcast;
        match h.frame_type {
            FrameType::Global => match Global::decode(h.command, &frame.payload) {
                Ok(Global::ReportAttributes(attributes)) => {
                    self.emit(HostEvent::Attributes {
                        ieee,
                        endpoint: aps.source_endpoint,
                        cluster: aps.cluster,
                        attributes,
                        lqi,
                    });
                }
                Ok(Global::ReadAttributesResponse(records)) => {
                    let attributes: Vec<(u16, Value)> = records
                        .into_iter()
                        .filter_map(|r| r.value.map(|v| (r.attribute, v)))
                        .collect();
                    self.emit(HostEvent::Attributes {
                        ieee,
                        endpoint: aps.source_endpoint,
                        cluster: aps.cluster,
                        attributes,
                        lqi,
                    });
                    wants_default_response = false;
                }
                Ok(Global::ReadAttributes(ids)) => {
                    self.answer_read(source, aps, h.seq, ids);
                    wants_default_response = false;
                }
                Ok(Global::DefaultResponse { .. }) => wants_default_response = false,
                Ok(_) => {}
                Err(e) => tracing::debug!(%ieee, error = %e, "malformed global command"),
            },
            FrameType::Cluster
                if aps.cluster == crate::ota::CLUSTER
                    && h.direction == Direction::ClientToServer
                    && h.manufacturer.is_none() =>
            {
                self.on_ota(ieee, source, aps, h.seq, h.command, &frame.payload);
                return;
            }
            FrameType::Cluster => {
                let from_server = h.direction == Direction::ServerToClient;
                let def = if h.manufacturer.is_none() {
                    zigbee_zcl::cluster(aps.cluster).and_then(|c| c.command(h.direction, h.command))
                } else {
                    None
                };
                let fields = def
                    .and_then(|d| d.decode(&frame.payload).ok())
                    .unwrap_or_default();
                if aps.cluster == zigbee_zcl::cluster_id::IAS_ZONE
                    && from_server
                    && h.command == zigbee_zcl::response::ias_zone::ZONE_ENROLL_REQUEST
                {
                    let host = self.clone();
                    let endpoint = aps.source_endpoint;
                    tokio::spawn(async move {
                        if let Err(e) =
                            crate::interview::enroll_response(&host, ieee, endpoint).await
                        {
                            tracing::warn!(%ieee, error = %e, "IAS enroll response");
                        }
                    });
                }
                self.emit(HostEvent::Command {
                    ieee,
                    endpoint: aps.source_endpoint,
                    cluster: aps.cluster,
                    command: h.command,
                    name: def.map(|d| d.name),
                    fields,
                    from_server,
                });
            }
        }
        if wants_default_response {
            let reply = Frame {
                header: zigbee_zcl::Header {
                    frame_type: FrameType::Global,
                    manufacturer: h.manufacturer,
                    direction: h.direction.reverse(),
                    disable_default_response: true,
                    seq: h.seq,
                    command: foundation::id::DEFAULT_RESPONSE,
                },
                payload: Global::DefaultResponse {
                    command: h.command,
                    status: zigbee_zcl::status::SUCCESS,
                }
                .encode(),
            };
            self.send_reply(source, aps, reply);
        }
    }

    /// Answer a device reading the host's attributes: the Time cluster, for
    /// devices that set their clock from the coordinator; everything else
    /// is unsupported.
    fn answer_read(&self, source: NodeId, aps: Aps, seq: u8, ids: Vec<u16>) {
        use zigbee_zcl::attr::time;
        let records = ids
            .into_iter()
            .map(|attribute| {
                let value = match (aps.cluster, attribute) {
                    (zigbee_zcl::cluster_id::TIME, time::TIME) => Some(Value::Uint(
                        zigbee_zcl::DataType::UtcTime,
                        u64::from(zcl_time_now()),
                    )),
                    (zigbee_zcl::cluster_id::TIME, time::TIME_STATUS) => Some(Value::map8(0x03)),
                    (zigbee_zcl::cluster_id::TIME, time::TIME_ZONE) => {
                        Some(Value::Int(zigbee_zcl::DataType::Int32, 0))
                    }
                    (zigbee_zcl::cluster_id::TIME, time::LOCAL_TIME) => {
                        Some(Value::u32(zcl_time_now()))
                    }
                    _ => None,
                };
                match value {
                    Some(v) => ReadRecord {
                        attribute,
                        status: zigbee_zcl::status::SUCCESS,
                        value: Some(v),
                    },
                    None => ReadRecord {
                        attribute,
                        status: zigbee_zcl::status::UNSUPPORTED_ATTRIBUTE,
                        value: None,
                    },
                }
            })
            .collect();
        let mut reply = Frame::global(
            Direction::ServerToClient,
            seq,
            foundation::id::READ_ATTRIBUTES_RESPONSE,
            Global::ReadAttributesResponse(records).encode(),
        );
        reply.header.disable_default_response = true;
        self.send_reply(source, aps, reply);
    }

    fn send_reply(&self, source: NodeId, aps: Aps, frame: Frame) {
        let host = self.clone();
        let reply_aps = Aps {
            profile: aps.profile,
            cluster: aps.cluster,
            source_endpoint: aps.destination_endpoint,
            destination_endpoint: aps.source_endpoint,
        };
        tokio::spawn(async move {
            if let Err(e) = host
                .inner
                .adapter
                .send_unicast(source, reply_aps, frame.encode())
                .await
            {
                tracing::debug!(error = %e, "sending a reply");
            }
        });
    }

    pub(crate) fn emit_event(&self, event: HostEvent) {
        self.emit(event);
    }

    pub(crate) fn config(&self) -> &HostConfig {
        &self.inner.config
    }

    pub(crate) fn mark_backup_wanted(&self) {
        self.state().backup_wanted = true;
    }

    pub(crate) fn used_zone_ids(&self) -> HashSet<u8> {
        self.state()
            .devices
            .values()
            .filter_map(|d| d.ias_zone_id)
            .collect()
    }
}
