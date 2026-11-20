//! The boundary between the host and a network co-processor family.
//!
//! The host does formation policy, the trust center's decisions, interviews
//! and everything above APS; an [`Adapter`] does what only the NCP can: start
//! or form the network, open it for joining, carry APS frames, and export
//! or import keys. EZSP is the first implementation
//! ([`crate::ezsp_adapter`]).

use std::future::Future;

use zigbee_types::{ExtPanId, Ieee, Key, NodeId, PanId};

use crate::backup::Backup;
use crate::error::HostError;

/// The network a coordinator should run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSettings {
    /// Channel, 11 to 26.
    pub channel: u8,
    /// PAN id.
    pub pan_id: PanId,
    /// Extended PAN id.
    pub extended_pan_id: ExtPanId,
    /// Network key.
    pub network_key: Key,
    /// Transmit power, dBm, if not the firmware's default.
    pub tx_power: Option<i8>,
}

/// How to start.
#[derive(Debug, Clone)]
pub enum StartMode {
    /// Resume the NCP's network if it is `settings`; form `settings` if the
    /// NCP has none. A different network on the NCP is an error unless
    /// `replace_other` (which leaves it, losing its devices).
    Normal {
        /// The network to run.
        settings: NetworkSettings,
        /// Replace a different network found on the NCP.
        replace_other: bool,
    },
    /// Leave whatever the NCP has and restore a backup onto it.
    Restore(Box<Backup>),
}

/// What [`Adapter::start`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    /// The NCP already ran the network.
    Resumed,
    /// A new network was formed.
    Formed,
    /// A backup was restored.
    Restored,
}

/// The running network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInfo {
    /// The coordinator's IEEE address.
    pub ieee: Ieee,
    /// The coordinator's network address (0).
    pub nwk: NodeId,
    /// Channel.
    pub channel: u8,
    /// PAN id.
    pub pan_id: PanId,
    /// Extended PAN id.
    pub extended_pan_id: ExtPanId,
    /// Network update id.
    pub nwk_update_id: u8,
}

/// The NCP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfo {
    /// Family, such as `ezsp`.
    pub kind: &'static str,
    /// Firmware version.
    pub firmware: String,
    /// Serial protocol version.
    pub protocol: u8,
    /// The NCP reports each device's link key exchange
    /// ([`AdapterEvent::KeyExchange`]), so the host can remove devices
    /// that never do one. Without it the NCP enforces
    /// [`JoinPolicy::allow_legacy`] itself, or cannot.
    pub reports_key_exchange: bool,
}

/// Trust center join policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinPolicy {
    /// Only devices with an install code may join.
    pub install_code_only: bool,
    /// Accept devices that never request a unique link key (pre-Zigbee 3.0
    /// devices keep the well-known key). When false they are removed.
    pub allow_legacy: bool,
}

impl Default for JoinPolicy {
    fn default() -> Self {
        JoinPolicy {
            install_code_only: false,
            allow_legacy: true,
        }
    }
}

/// APS addressing for a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aps {
    /// Profile.
    pub profile: u16,
    /// Cluster.
    pub cluster: u16,
    /// Source endpoint.
    pub source_endpoint: u8,
    /// Destination endpoint.
    pub destination_endpoint: u8,
}

/// Something the NCP reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterEvent {
    /// An APS frame for the host.
    Message {
        /// Sender's network address.
        source: NodeId,
        /// Sender's IEEE address, when the frame carried it.
        source_ieee: Option<Ieee>,
        /// APS addressing.
        aps: Aps,
        /// Sent as a broadcast or multicast.
        broadcast: bool,
        /// The group, for a multicast.
        group: Option<u16>,
        /// Last hop link quality.
        lqi: u8,
        /// Last hop signal strength, dBm, when the NCP reports it.
        rssi: Option<i8>,
        /// The APS payload.
        payload: Vec<u8>,
    },
    /// A device joined or rejoined.
    Joined {
        /// Network address.
        nwk: NodeId,
        /// IEEE address.
        ieee: Ieee,
        /// Its parent.
        parent: NodeId,
        /// A rejoin (the device was on the network before).
        rejoin: bool,
    },
    /// A device left.
    Left {
        /// Network address.
        nwk: NodeId,
        /// IEEE address.
        ieee: Ieee,
    },
    /// Trust center link key exchange with a device finished.
    KeyExchange {
        /// The device.
        ieee: Ieee,
        /// It now has a unique link key.
        success: bool,
    },
    /// A route record arrived.
    RouteRecord {
        /// The originator.
        source: NodeId,
        /// Its IEEE address, when the NCP reports it.
        ieee: Option<Ieee>,
        /// Relays, nearest the source first.
        relays: Vec<NodeId>,
        /// Last hop link quality, when the NCP reports it.
        lqi: Option<u8>,
        /// Last hop signal strength, when the NCP reports it.
        rssi: Option<i8>,
    },
    /// A frame given to [`Adapter::send_unicast`] was delivered (APS
    /// acknowledged) or not.
    Delivery {
        /// The tag `send_unicast` returned.
        tag: u16,
        /// Delivered.
        delivered: bool,
    },
    /// The network went down.
    NetworkDown,
    /// The network came up.
    NetworkUp,
    /// The adapter failed (NCP reset or port lost); restart the host.
    Failed(String),
}

/// A network co-processor family. Every method's future is `Send` so the
/// host can call it from any task.
pub trait Adapter: Send + Sync + 'static {
    /// The NCP's identity.
    fn info(&self) -> AdapterInfo;

    /// Configure the NCP and bring the network up.
    fn start(
        &self,
        mode: StartMode,
    ) -> impl Future<Output = Result<(StartOutcome, NetworkInfo), HostError>> + Send;

    /// Open (seconds > 0) or close (0) the coordinator for joining.
    fn permit_join(
        &self,
        seconds: u8,
        policy: &JoinPolicy,
    ) -> impl Future<Output = Result<(), HostError>> + Send;

    /// Let the device `ieee` join with `key` (from its install code).
    fn add_join_key(
        &self,
        ieee: Ieee,
        key: Key,
    ) -> impl Future<Output = Result<(), HostError>> + Send;

    /// Send an APS unicast. Returns a tag that a later
    /// [`AdapterEvent::Delivery`] names.
    fn send_unicast(
        &self,
        nwk: NodeId,
        aps: Aps,
        payload: Vec<u8>,
    ) -> impl Future<Output = Result<u16, HostError>> + Send;

    /// Send an APS broadcast to 0xFFFF, 0xFFFD or 0xFFFC.
    fn send_broadcast(
        &self,
        destination: NodeId,
        aps: Aps,
        payload: Vec<u8>,
    ) -> impl Future<Output = Result<(), HostError>> + Send;

    /// Read the running network's parameters again (after a channel
    /// change).
    fn network_info(&self) -> impl Future<Output = Result<NetworkInfo, HostError>> + Send;

    /// Export everything a restore needs.
    fn backup(&self) -> impl Future<Output = Result<Backup, HostError>> + Send;

    /// Measure each channel's energy; returns (channel, max RSSI dBm).
    fn energy_scan(
        &self,
        channel_mask: u32,
    ) -> impl Future<Output = Result<Vec<(u8, i8)>, HostError>> + Send;
}
