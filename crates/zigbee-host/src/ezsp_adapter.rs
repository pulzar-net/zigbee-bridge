//! The [`Adapter`] for Silicon Labs EmberZNet NCPs over EZSP.
//!
//! Security follows Zigbee 3.0 with hashed trust center link keys: joining
//! devices use the well-known key (or their install code key) to receive
//! the network key, then ask the trust center for a link key of their own,
//! which the NCP derives from a random root key and the device's IEEE
//! address. The root key is in every backup, so the derived keys survive a
//! restore without a key table entry per device.

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, watch};
use zigbee_ezsp::client::{ClientConfig, Ezsp, EzspEvent, NcpInfo};
use zigbee_ezsp::ids::{
    self, aps_option, config, decision, ext_security, policy, security, tc_decision, value,
};
use zigbee_ezsp::types::{InitialSecurityState, NetworkParameters, SecManContext};
use zigbee_ezsp::{ApsFrame, Callback, Command, Response, Status};
use zigbee_types::{ExtPanId, Ieee, Key, NodeId};

use crate::adapter::{
    Adapter, AdapterEvent, AdapterInfo, Aps, JoinPolicy, NetworkInfo, NetworkSettings, StartMode,
    StartOutcome,
};
use crate::backup::{
    self, Backup, BackupDevice, EzspSpecific, Internal, LinkKey, Metadata, NetworkKey,
    StackSpecific,
};
use crate::error::HostError;

/// How long to wait for the network to come up or go down.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(10);
/// Silicon Labs' manufacturer code, which the coordinator reports.
const MANUFACTURER_CODE: u16 = 0x1049;
/// Every 2.4 GHz channel.
const ALL_CHANNELS: u32 = 0x07FF_F800;
/// Default APS options for unicasts.
const UNICAST_OPTIONS: u16 =
    aps_option::RETRY | aps_option::ENABLE_ROUTE_DISCOVERY | aps_option::ENABLE_ADDRESS_DISCOVERY;

/// The clusters the host's endpoint 1 serves and uses; devices bind their
/// reporting clusters to it.
const ENDPOINT_INPUT_CLUSTERS: &[u16] = &[
    0x0000, 0x0003, 0x0006, 0x000A, 0x0019, 0x0300, 0x0402, 0x0405, 0x0500, 0x0702, 0x0B04,
];
const ENDPOINT_OUTPUT_CLUSTERS: &[u16] = &[
    0x0000, 0x0001, 0x0003, 0x0004, 0x0005, 0x0006, 0x0008, 0x000F, 0x0012, 0x0020, 0x0101, 0x0102,
    0x0201, 0x0202, 0x0300, 0x0400, 0x0402, 0x0403, 0x0405, 0x0406, 0x0500, 0x0502, 0x0702, 0x0B04,
    0x0B05,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetState {
    Unknown,
    Up,
    Down,
}

type ScanResult = Result<Vec<(u8, i8)>, HostError>;

struct ScanCollector {
    results: Vec<(u8, i8)>,
    done: Option<tokio::sync::oneshot::Sender<ScanResult>>,
}

/// An EmberZNet NCP.
pub struct EzspAdapter {
    ezsp: Ezsp,
    info: NcpInfo,
    net: watch::Receiver<NetState>,
    scan: Arc<Mutex<Option<ScanCollector>>>,
    tag: AtomicU16,
}

impl std::fmt::Debug for EzspAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EzspAdapter")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn translate(
    cb: Callback,
    net: &watch::Sender<NetState>,
    scan: &Mutex<Option<ScanCollector>>,
) -> Option<AdapterEvent> {
    match cb {
        Callback::StackStatus { status } => {
            if status.is_network_up() {
                let _ = net.send(NetState::Up);
                Some(AdapterEvent::NetworkUp)
            } else if status.is_network_down() {
                let _ = net.send(NetState::Down);
                Some(AdapterEvent::NetworkDown)
            } else {
                tracing::debug!(%status, "stack status");
                None
            }
        }
        Callback::IncomingMessage {
            kind,
            aps,
            info,
            payload,
        } => Some(AdapterEvent::Message {
            source: info.sender,
            source_ieee: info.sender_ieee,
            aps: Aps {
                profile: aps.profile_id,
                cluster: aps.cluster_id,
                source_endpoint: aps.source_endpoint,
                destination_endpoint: aps.destination_endpoint,
            },
            broadcast: !matches!(kind, ids::incoming::UNICAST | ids::incoming::UNICAST_REPLY),
            group: matches!(
                kind,
                ids::incoming::MULTICAST | ids::incoming::MULTICAST_LOOPBACK
            )
            .then_some(aps.group_id),
            lqi: info.last_hop_lqi,
            rssi: Some(info.last_hop_rssi),
            payload,
        }),
        Callback::TrustCenterJoin {
            node_id,
            ieee,
            status,
            decision,
            parent,
        } => match status {
            ids::device_update::DEVICE_LEFT => Some(AdapterEvent::Left { nwk: node_id, ieee }),
            _ if decision == ids::join_decision::DENY_JOIN => {
                tracing::info!(%ieee, "trust center denied a join");
                None
            }
            ids::device_update::UNSECURED_JOIN => Some(AdapterEvent::Joined {
                nwk: node_id,
                ieee,
                parent,
                rejoin: false,
            }),
            _ => Some(AdapterEvent::Joined {
                nwk: node_id,
                ieee,
                parent,
                rejoin: true,
            }),
        },
        Callback::ZigbeeKeyEstablishment { partner, status } => {
            use ids::key_status::*;
            let success = matches!(
                status,
                TRUST_CENTER_LINK_KEY_ESTABLISHED
                    | TC_RESPONDED_TO_KEY_REQUEST
                    | VERIFY_LINK_KEY_SUCCESS
            );
            tracing::debug!(%partner, status, "key establishment");
            Some(AdapterEvent::KeyExchange {
                ieee: partner,
                success,
            })
        }
        Callback::IncomingRouteRecord {
            source,
            ieee,
            last_hop_lqi,
            last_hop_rssi,
            relays,
        } => Some(AdapterEvent::RouteRecord {
            source,
            ieee: Some(ieee),
            relays,
            lqi: Some(last_hop_lqi),
            rssi: Some(last_hop_rssi),
        }),
        Callback::MessageSent { status, tag, .. } => Some(AdapterEvent::Delivery {
            tag,
            delivered: status.is_ok(),
        }),
        Callback::EnergyScanResult { channel, max_rssi } => {
            if let Ok(mut s) = scan.lock() {
                if let Some(c) = s.as_mut() {
                    c.results.push((channel, max_rssi));
                }
            }
            None
        }
        Callback::ScanComplete { status, channel } => {
            if let Some(mut c) = scan.lock().ok().and_then(|mut s| s.take()) {
                let result = if status.is_ok() {
                    Ok(std::mem::take(&mut c.results))
                } else {
                    Err(HostError::Ncp(format!(
                        "scan failed on channel {channel}: {status}"
                    )))
                };
                if let Some(done) = c.done.take() {
                    let _ = done.send(result);
                }
            }
            None
        }
        Callback::IdConflict { node_id } => {
            tracing::warn!(node_id, "network address conflict");
            None
        }
        other => {
            tracing::trace!(?other, "unhandled callback");
            None
        }
    }
}

impl EzspAdapter {
    /// Connect to an NCP over `io`. Returns the adapter and its events.
    pub async fn connect<T>(
        io: T,
        config: ClientConfig,
    ) -> Result<(EzspAdapter, mpsc::UnboundedReceiver<AdapterEvent>), HostError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (ezsp, info, mut ezsp_events) = Ezsp::start(io, config).await?;
        tracing::info!(protocol = info.protocol.0, firmware = %info.stack_version, "EZSP NCP connected");
        let (net_tx, net) = watch::channel(NetState::Unknown);
        let scan: Arc<Mutex<Option<ScanCollector>>> = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::unbounded_channel();
        let scan_for_task = scan.clone();
        tokio::spawn(async move {
            while let Some(event) = ezsp_events.recv().await {
                let out = match event {
                    EzspEvent::Callback(cb) => translate(cb, &net_tx, &scan_for_task),
                    EzspEvent::Down(why) => Some(AdapterEvent::Failed(why)),
                };
                if let Some(out) = out {
                    if tx.send(out).is_err() {
                        break;
                    }
                }
            }
        });
        Ok((
            EzspAdapter {
                ezsp,
                info,
                net,
                scan,
                tag: AtomicU16::new(1),
            },
            rx,
        ))
    }

    /// The NCP's start-up report.
    pub fn ncp_info(&self) -> &NcpInfo {
        &self.info
    }

    async fn wait_net(&self, want: NetState) -> Result<(), HostError> {
        let mut net = self.net.clone();
        let reached = matches!(
            tokio::time::timeout(NETWORK_TIMEOUT, net.wait_for(|s| *s == want)).await,
            Ok(Ok(_))
        );
        if reached {
            Ok(())
        } else {
            Err(HostError::Ncp(format!(
                "network did not go {}",
                if want == NetState::Up { "up" } else { "down" }
            )))
        }
    }

    async fn ok(&self, command: Command) -> Result<(), HostError> {
        self.ezsp.call_ok(command).await.map_err(HostError::from)
    }

    /// Configuration that must be set after every NCP reset, before the
    /// network starts.
    async fn configure(&self) -> Result<(), HostError> {
        let required = [
            (config::STACK_PROFILE, ids::STACK_PROFILE_ZIGBEE_PRO),
            (config::SECURITY_LEVEL, ids::SECURITY_LEVEL_Z3),
            (config::SUPPORTED_NETWORKS, 1),
        ];
        for (id, v) in required {
            self.ezsp.set_config(id, v).await?;
        }
        // Tuning the firmware may refuse (its build fixes some sizes).
        let tuning = [
            (config::MAX_HOPS, 30),
            (config::INDIRECT_TRANSMISSION_TIMEOUT, 7680),
            (config::TRUST_CENTER_ADDRESS_CACHE_SIZE, 2),
            (config::MAX_END_DEVICE_CHILDREN, 32),
            (config::END_DEVICE_POLL_TIMEOUT, 8),
            (config::TRANSIENT_KEY_TIMEOUT_S, 300),
        ];
        for (id, v) in tuning {
            if let Err(e) = self.ezsp.set_config(id, v).await {
                tracing::warn!(config_id = id, value = v, error = %e, "NCP kept its own configuration value");
            }
        }
        self.ezsp
            .set_policy(
                policy::BINDING_MODIFICATION,
                decision::CHECK_BINDING_MODIFICATIONS_ARE_VALID_ENDPOINT_CLUSTERS,
            )
            .await?;
        self.ezsp
            .set_policy(
                policy::MESSAGE_CONTENTS_IN_CALLBACK,
                decision::MESSAGE_TAG_ONLY_IN_CALLBACK,
            )
            .await?;
        self.ezsp
            .set_policy(
                policy::TC_KEY_REQUEST,
                decision::ALLOW_TC_KEY_REQUESTS_AND_SEND_CURRENT_KEY,
            )
            .await?;
        self.ezsp
            .set_policy(policy::APP_KEY_REQUEST, decision::DENY_APP_KEY_REQUESTS)
            .await?;
        self.ezsp
            .set_policy(policy::TRUST_CENTER, tc_decision::ALLOW_UNSECURED_REJOINS)
            .await?;
        self.ok(Command::SetManufacturerCode {
            code: MANUFACTURER_CODE,
        })
        .await?;
        let endpoint = Command::AddEndpoint {
            endpoint: 1,
            profile_id: zigbee_zcl::PROFILE_HOME_AUTOMATION,
            device_id: 0x0005,
            device_version: 0,
            input_clusters: ENDPOINT_INPUT_CLUSTERS.to_vec(),
            output_clusters: ENDPOINT_OUTPUT_CLUSTERS.to_vec(),
        };
        if let Err(e) = self.ok(endpoint).await {
            // After a soft restart without a reset the endpoint exists.
            tracing::warn!(error = %e, "adding endpoint 1");
        }
        Ok(())
    }

    async fn network_parameters(&self) -> Result<(u8, NetworkParameters), HostError> {
        match self.ezsp.call(Command::GetNetworkParameters).await? {
            Response::GetNetworkParameters {
                status,
                node_type,
                parameters,
            } if status.is_ok() => Ok((node_type, parameters)),
            other => Err(HostError::Ncp(format!("getNetworkParameters: {other:?}"))),
        }
    }

    async fn export_key(&self, key_type: u8) -> Result<Key, HostError> {
        match self
            .ezsp
            .call(Command::ExportKey {
                context: SecManContext::for_key(key_type),
            })
            .await?
        {
            Response::ExportKey { status, key, .. } if status.is_ok() => Ok(key),
            other => Err(HostError::Ncp(format!("exportKey({key_type}): {other:?}"))),
        }
    }

    async fn eui64(&self) -> Result<Ieee, HostError> {
        match self.ezsp.call(Command::GetEui64).await? {
            Response::GetEui64 { ieee } => Ok(ieee),
            other => Err(HostError::Ncp(format!("getEui64: {other:?}"))),
        }
    }

    async fn leave(&self) -> Result<(), HostError> {
        self.ok(Command::LeaveNetwork { options: 0 }).await?;
        self.wait_net(NetState::Down).await?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(())
    }

    async fn form(
        &self,
        settings: &NetworkSettings,
        tc_root: Key,
        restore: Option<&Backup>,
    ) -> Result<(), HostError> {
        let mut bitmask = security::TRUST_CENTER_GLOBAL_LINK_KEY
            | security::HAVE_PRECONFIGURED_KEY
            | security::HAVE_NETWORK_KEY
            | security::TRUST_CENTER_USES_HASHED_LINK_KEY
            | security::REQUIRE_ENCRYPTED_KEY;
        let mut sequence = 0;
        if let Some(backup) = restore {
            bitmask |= security::NO_FRAME_COUNTER_RESET;
            sequence = backup.network_key.sequence_number;
            self.ezsp
                .set_value(
                    value::NWK_FRAME_COUNTER,
                    backup.restore_frame_counter().to_le_bytes().to_vec(),
                )
                .await?;
        }
        self.ok(Command::SetInitialSecurityState {
            state: InitialSecurityState {
                bitmask,
                preconfigured_key: tc_root,
                network_key: settings.network_key,
                network_key_sequence: sequence,
                preconfigured_trust_center: Ieee::ZERO,
            },
        })
        .await?;
        let ext =
            ext_security::JOINER_GLOBAL_LINK_KEY | ext_security::NWK_LEAVE_REQUEST_NOT_ALLOWED;
        self.ezsp
            .set_value(value::EXTENDED_SECURITY_BITMASK, ext.to_le_bytes().to_vec())
            .await?;
        if restore.is_none() {
            if let Err(e) = self.ok(Command::ClearKeyTable).await {
                tracing::warn!(error = %e, "clearing the key table");
            }
        }
        let parameters = NetworkParameters {
            extended_pan_id: settings.extended_pan_id,
            pan_id: settings.pan_id,
            radio_tx_power: settings.tx_power.unwrap_or(5),
            radio_channel: settings.channel,
            join_method: 0,
            nwk_manager_id: 0,
            nwk_update_id: restore.map(|b| b.nwk_update_id).unwrap_or(0),
            channels: ALL_CHANNELS,
        };
        self.ok(Command::FormNetwork { parameters }).await?;
        self.wait_net(NetState::Up).await?;
        if let Err(e) = self
            .ezsp
            .set_value(value::STACK_TOKEN_WRITING, vec![1])
            .await
        {
            tracing::warn!(error = %e, "enabling stack token writing");
        }
        Ok(())
    }

    async fn restore(&self, backup: &Backup) -> Result<(), HostError> {
        let ieee = self.eui64().await?;
        if ieee != backup.coordinator_ieee {
            // The custom EUI-64 token can be written once; all ones means unset.
            let token = match self
                .ezsp
                .call(Command::GetMfgToken {
                    token_id: ids::mfg_token::CUSTOM_EUI_64,
                })
                .await?
            {
                Response::GetMfgToken { data } => data,
                other => return Err(HostError::Ncp(format!("getMfgToken: {other:?}"))),
            };
            if token.len() != 8 || token.iter().any(|b| *b != 0xFF) {
                return Err(HostError::RestoreRefused(format!(
                    "this coordinator is {ieee} and its IEEE address token is already written; the backup needs {}",
                    backup.coordinator_ieee
                )));
            }
            self.ok(Command::SetMfgToken {
                token_id: ids::mfg_token::CUSTOM_EUI_64,
                data: backup.coordinator_ieee.to_le_bytes().to_vec(),
            })
            .await?;
            return Err(HostError::RestartRequired);
        }
        let key_table = self
            .ezsp
            .get_config(config::KEY_TABLE_SIZE)
            .await
            .unwrap_or(0);
        let keys: Vec<(Ieee, &LinkKey)> = backup
            .devices
            .iter()
            .filter_map(|d| d.link_key.as_ref().map(|k| (d.ieee_address, k)))
            .collect();
        if keys.len() > usize::from(key_table) {
            return Err(HostError::RestoreRefused(format!(
                "{} link keys do not fit the NCP's key table of {key_table}",
                keys.len()
            )));
        }
        for index in 0..key_table {
            let result = match keys.get(usize::from(index)) {
                Some((ieee, link)) => {
                    self.ok(Command::ImportLinkKey {
                        index: index as u8,
                        ieee: *ieee,
                        key: link.key,
                    })
                    .await
                }
                None => {
                    self.ok(Command::EraseKeyTableEntry { index: index as u8 })
                        .await
                }
            };
            if let Err(e) = result {
                tracing::warn!(index, error = %e, "restoring key table entry");
            }
        }
        // A backup from another family has no root key: a new one, with the
        // devices' own link keys (if the backup has them) in the key table.
        let tc_root = match backup.stack_specific.as_ref().and_then(|s| s.ezsp.as_ref()) {
            Some(e) => e.hashed_tclk,
            None => {
                tracing::info!(
                    keys = keys.len(),
                    "restoring a backup from another coordinator family"
                );
                random_key()?
            }
        };
        let settings = NetworkSettings {
            channel: backup.channel,
            pan_id: backup.pan_id,
            extended_pan_id: backup.extended_pan_id,
            network_key: backup.network_key.key,
            tx_power: None,
        };
        self.form(&settings, tc_root, Some(backup)).await
    }

    async fn after_up(&self, settings_power: Option<i8>) -> Result<NetworkInfo, HostError> {
        self.ok(Command::SetConcentrator {
            on: true,
            kind: ids::concentrator::HIGH_RAM,
            min_time: 5,
            max_time: 60,
            route_error_threshold: 3,
            delivery_failure_threshold: 1,
            max_hops: 0,
        })
        .await?;
        if let Err(e) = self
            .ezsp
            .call(Command::SetSourceRouteDiscoveryMode { mode: 2 })
            .await
        {
            tracing::warn!(error = %e, "source route discovery mode");
        }
        if let Some(power) = settings_power {
            if let Err(e) = self.ok(Command::SetRadioPower { power }).await {
                tracing::warn!(power, error = %e, "setting radio power");
            }
        }
        self.read_network_info().await
    }

    async fn read_network_info(&self) -> Result<NetworkInfo, HostError> {
        let (_, p) = self.network_parameters().await?;
        let nwk = match self.ezsp.call(Command::GetNodeId).await? {
            Response::GetNodeId { node_id } => node_id,
            _ => 0,
        };
        Ok(NetworkInfo {
            ieee: self.eui64().await?,
            nwk,
            channel: p.radio_channel,
            pan_id: p.pan_id,
            extended_pan_id: p.extended_pan_id,
            nwk_update_id: p.nwk_update_id,
        })
    }

    fn next_tag(&self) -> u16 {
        // Protocol 13 carries one-byte tags.
        let tag = self.tag.fetch_add(1, Ordering::Relaxed);
        if self.ezsp.version().sl_status() {
            tag
        } else {
            tag & 0xFF
        }
    }
}

fn describe(p: &NetworkParameters) -> String {
    format!(
        "channel {}, PAN {:04x}, extended PAN {}",
        p.radio_channel, p.pan_id, p.extended_pan_id
    )
}

fn random_key() -> Result<Key, HostError> {
    let mut k = [0u8; 16];
    getrandom::fill(&mut k).map_err(|e| HostError::Io(format!("random: {e}")))?;
    Ok(Key(k))
}

impl Adapter for EzspAdapter {
    fn info(&self) -> AdapterInfo {
        let v = self.info.stack_version;
        AdapterInfo {
            kind: "ezsp",
            firmware: format!("{}.{}.{}.{}", v.major, v.minor, v.patch, v.special),
            protocol: self.info.protocol.0,
            reports_key_exchange: true,
        }
    }

    async fn start(&self, mode: StartMode) -> Result<(StartOutcome, NetworkInfo), HostError> {
        self.configure().await?;
        let init = self
            .ezsp
            .call(Command::NetworkInit {
                bitmask: ids::network_init::PARENT_INFO_IN_TOKEN
                    | ids::network_init::END_DEVICE_REJOIN_ON_REBOOT,
            })
            .await?;
        let status = init.status().unwrap_or(Status::SL_OK);
        let has_network = status.is_ok();
        if !has_network && !status.is_not_joined() {
            return Err(HostError::Ncp(format!("networkInit: {status}")));
        }
        if has_network {
            self.wait_net(NetState::Up).await?;
        }

        match mode {
            StartMode::Restore(backup) => {
                if has_network {
                    self.leave().await?;
                }
                self.restore(&backup).await?;
                Ok((StartOutcome::Restored, self.after_up(None).await?))
            }
            StartMode::Normal {
                settings,
                replace_other,
            } => {
                if has_network {
                    let (node_type, p) = self.network_parameters().await?;
                    let key = self.export_key(ids::key_type::NETWORK).await?;
                    let same = node_type == ids::node_type::COORDINATOR
                        && p.pan_id == settings.pan_id
                        && p.extended_pan_id == settings.extended_pan_id
                        && key == settings.network_key;
                    if same {
                        return Ok((
                            StartOutcome::Resumed,
                            self.after_up(settings.tx_power).await?,
                        ));
                    }
                    if !replace_other {
                        return Err(HostError::NetworkMismatch {
                            found: describe(&p),
                        });
                    }
                    tracing::warn!(found = %describe(&p), "leaving the NCP's other network, as configured");
                    self.leave().await?;
                }
                self.form(&settings, random_key()?, None).await?;
                Ok((
                    StartOutcome::Formed,
                    self.after_up(settings.tx_power).await?,
                ))
            }
        }
    }

    async fn permit_join(&self, seconds: u8, join: &JoinPolicy) -> Result<(), HostError> {
        if seconds > 0 {
            if !join.install_code_only {
                // The well-known key for any device, while open.
                self.ok(Command::ImportTransientKey {
                    ieee: Ieee::ZERO,
                    key: Key::ZIGBEE_ALLIANCE_09,
                })
                .await?;
            }
            let mut decision_bits = tc_decision::ALLOW_JOINS | tc_decision::ALLOW_UNSECURED_REJOINS;
            if join.install_code_only {
                decision_bits |= tc_decision::JOINS_USE_INSTALL_CODE_KEY;
            }
            self.ezsp
                .set_policy(policy::TRUST_CENTER, decision_bits)
                .await?;
        } else {
            self.ezsp
                .set_policy(policy::TRUST_CENTER, tc_decision::ALLOW_UNSECURED_REJOINS)
                .await?;
        }
        self.ok(Command::PermitJoining { duration: seconds })
            .await?;
        if seconds == 0 {
            self.ok(Command::ClearTransientLinkKeys).await?;
        }
        Ok(())
    }

    async fn add_join_key(&self, ieee: Ieee, key: Key) -> Result<(), HostError> {
        self.ok(Command::ImportTransientKey { ieee, key }).await
    }

    async fn send_unicast(
        &self,
        nwk: NodeId,
        aps: Aps,
        payload: Vec<u8>,
    ) -> Result<u16, HostError> {
        let tag = self.next_tag();
        let frame = ApsFrame {
            profile_id: aps.profile,
            cluster_id: aps.cluster,
            source_endpoint: aps.source_endpoint,
            destination_endpoint: aps.destination_endpoint,
            options: UNICAST_OPTIONS,
            group_id: 0,
            sequence: 0,
        };
        match self
            .ezsp
            .call(Command::SendUnicast {
                kind: ids::outgoing::DIRECT,
                destination: nwk,
                aps: frame,
                tag,
                payload,
            })
            .await?
        {
            Response::Sent { status, .. } if status.is_ok() => Ok(tag),
            Response::Sent { status, .. } => Err(HostError::Ncp(format!(
                "sendUnicast to {nwk:#06x}: {status}"
            ))),
            other => Err(HostError::Ncp(format!("sendUnicast: {other:?}"))),
        }
    }

    async fn send_broadcast(
        &self,
        destination: NodeId,
        aps: Aps,
        payload: Vec<u8>,
    ) -> Result<(), HostError> {
        let tag = self.next_tag();
        let frame = ApsFrame {
            profile_id: aps.profile,
            cluster_id: aps.cluster,
            source_endpoint: aps.source_endpoint,
            destination_endpoint: aps.destination_endpoint,
            options: aps_option::NONE,
            group_id: 0,
            sequence: 0,
        };
        let command = Command::SendBroadcast {
            alias: 0,
            destination,
            nwk_sequence: 0,
            aps: frame,
            radius: 30,
            tag,
            payload,
        };
        match self.ezsp.call(command).await? {
            Response::Sent { status, .. } if status.is_ok() => Ok(()),
            other => Err(HostError::Ncp(format!("sendBroadcast: {other:?}"))),
        }
    }

    async fn network_info(&self) -> Result<NetworkInfo, HostError> {
        self.read_network_info().await
    }

    async fn backup(&self) -> Result<Backup, HostError> {
        let (_, p) = self.network_parameters().await?;
        let network_key = self.export_key(ids::key_type::NETWORK).await?;
        let tc_root = self.export_key(ids::key_type::TC_LINK).await?;
        let (sequence_number, frame_counter) =
            match self.ezsp.call(Command::GetNetworkKeyInfo).await? {
                Response::GetNetworkKeyInfo { status, info } if status.is_ok() => {
                    (info.network_key_sequence, info.network_key_frame_counter)
                }
                other => return Err(HostError::Ncp(format!("getNetworkKeyInfo: {other:?}"))),
            };
        let key_table = self
            .ezsp
            .get_config(config::KEY_TABLE_SIZE)
            .await
            .unwrap_or(0);
        let mut devices = Vec::new();
        for index in 0..key_table.min(255) {
            if let Ok(Response::ExportLinkKeyByIndex {
                status,
                context,
                key,
                metadata,
            }) = self
                .ezsp
                .call(Command::ExportLinkKeyByIndex { index: index as u8 })
                .await
            {
                if status.is_ok() && context.ieee != Ieee::ZERO && context.ieee != Ieee::UNSET {
                    devices.push(BackupDevice {
                        nwk_address: None,
                        ieee_address: context.ieee,
                        is_child: false,
                        link_key: Some(LinkKey {
                            key,
                            tx_counter: metadata.outgoing_frame_counter,
                            rx_counter: metadata.incoming_frame_counter,
                        }),
                    });
                }
            }
        }
        let firmware = self.info().firmware;
        Ok(Backup {
            metadata: Metadata {
                format: backup::FORMAT.into(),
                version: 1,
                source: concat!("zigbee-bridge@", env!("CARGO_PKG_VERSION")).into(),
                internal: Internal {
                    created_unix: Some(now_unix()),
                    ezsp_version: Some(self.info.protocol.0),
                    firmware: Some(firmware),
                },
            },
            stack_specific: Some(StackSpecific {
                ezsp: Some(EzspSpecific {
                    hashed_tclk: tc_root,
                }),
                zstack: None,
            }),
            coordinator_ieee: self.eui64().await?,
            pan_id: p.pan_id,
            extended_pan_id: p.extended_pan_id,
            nwk_update_id: p.nwk_update_id,
            security_level: 5,
            channel: p.radio_channel,
            channel_mask: (11..=26).filter(|c| p.channels & (1 << c) != 0).collect(),
            network_key: NetworkKey {
                key: network_key,
                sequence_number,
                frame_counter,
            },
            devices,
        })
    }

    async fn energy_scan(&self, channel_mask: u32) -> Result<Vec<(u8, i8)>, HostError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Ok(mut s) = self.scan.lock() {
            if s.is_some() {
                return Err(HostError::Invalid("a scan is already running".into()));
            }
            *s = Some(ScanCollector {
                results: Vec::new(),
                done: Some(tx),
            });
        }
        let started = self
            .ezsp
            .call_ok(Command::StartScan {
                scan_type: ids::scan_type::ENERGY,
                channel_mask,
                duration: 3,
            })
            .await;
        if let Err(e) = started {
            if let Ok(mut s) = self.scan.lock() {
                *s = None;
            }
            return Err(e.into());
        }
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(result)) => result,
            _ => {
                if let Ok(mut s) = self.scan.lock() {
                    *s = None;
                }
                Err(HostError::Ncp("energy scan did not complete".into()))
            }
        }
    }
}

/// A fresh random network: channel `channel`, random PAN ids and network
/// key. For a platform that manages keys itself, build
/// [`NetworkSettings`] from its own values instead.
pub fn random_network(channel: u8) -> Result<NetworkSettings, HostError> {
    let mut pan = [0u8; 2];
    let mut ext = [0u8; 8];
    getrandom::fill(&mut pan).map_err(|e| HostError::Io(format!("random: {e}")))?;
    getrandom::fill(&mut ext).map_err(|e| HostError::Io(format!("random: {e}")))?;
    // 0x0000 and 0xFFF8 to 0xFFFF are reserved PAN ids.
    let pan_id = u16::from_le_bytes(pan) % 0xFFF7 + 1;
    Ok(NetworkSettings {
        channel,
        pan_id,
        extended_pan_id: ExtPanId(u64::from_le_bytes(ext)),
        network_key: random_key()?,
        tx_power: None,
    })
}
