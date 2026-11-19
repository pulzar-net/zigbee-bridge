//! What the host knows about each device.

use serde::{Deserialize, Serialize};
use zigbee_types::{Ieee, NodeId};
use zigbee_zdo::{NodeDescriptor, SimpleDescriptor};

/// The interview's steps, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterviewStage {
    /// Read the node descriptor.
    NodeDescriptor,
    /// List the active endpoints.
    ActiveEndpoints,
    /// Read each endpoint's simple descriptor.
    SimpleDescriptors,
    /// Read the Basic cluster (manufacturer, model, power source).
    Basic,
    /// Bind clusters, configure reporting, enroll IAS zones.
    Configure,
    /// Finished.
    Done,
}

impl InterviewStage {
    /// The next step.
    pub fn next(self) -> InterviewStage {
        match self {
            InterviewStage::NodeDescriptor => InterviewStage::ActiveEndpoints,
            InterviewStage::ActiveEndpoints => InterviewStage::SimpleDescriptors,
            InterviewStage::SimpleDescriptors => InterviewStage::Basic,
            InterviewStage::Basic => InterviewStage::Configure,
            InterviewStage::Configure | InterviewStage::Done => InterviewStage::Done,
        }
    }
}

/// Where a device's interview stands. Persisted, so an interview a sleepy
/// device did not finish resumes where it stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interview {
    /// The next step to run ([`InterviewStage::Done`] when complete).
    pub stage: InterviewStage,
    /// The last failure, if the interview stopped on one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Attempts so far.
    #[serde(default)]
    pub attempts: u32,
}

impl Default for Interview {
    fn default() -> Self {
        Interview {
            stage: InterviewStage::NodeDescriptor,
            error: None,
            attempts: 0,
        }
    }
}

impl Interview {
    /// True once every step has run.
    pub fn is_done(&self) -> bool {
        self.stage == InterviewStage::Done
    }
}

/// A reporting configuration the host wrote to a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfiguredReport {
    /// Endpoint.
    pub endpoint: u8,
    /// Cluster.
    pub cluster: u16,
    /// Attribute.
    pub attribute: u16,
    /// The device's ZCL status for it (0 is success).
    pub status: u8,
}

/// A device on the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// IEEE address.
    pub ieee: Ieee,
    /// Current network address.
    pub nwk: NodeId,
    /// A name a person gave it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The node descriptor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_descriptor: Option<NodeDescriptor>,
    /// Endpoints, from the simple descriptors.
    #[serde(default)]
    pub endpoints: Vec<SimpleDescriptor>,
    /// Basic cluster: manufacturer name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    /// Basic cluster: model identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Basic cluster: power source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_source: Option<u8>,
    /// Basic cluster: software build id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sw_build_id: Option<String>,
    /// Basic cluster: date code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_code: Option<String>,
    /// Basic cluster: application version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<u8>,
    /// The interview's progress.
    #[serde(default)]
    pub interview: Interview,
    /// Reporting the host configured.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reporting: Vec<ConfiguredReport>,
    /// The IAS zone type (contact, motion, water leak...), read at enrollment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ias_zone_type: Option<u16>,
    /// The IAS zone id the host enrolled it with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ias_zone_id: Option<u8>,
    /// It completed Zigbee 3.0 trust center link key exchange.
    #[serde(default)]
    pub unique_link_key: bool,
    /// Its parent when it joined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<NodeId>,
    /// When it joined, seconds since the Unix epoch.
    #[serde(default)]
    pub joined_unix: u64,
    /// Last time anything arrived from it (not persisted, so receiving a
    /// message never writes to flash).
    #[serde(skip)]
    pub last_seen_unix: Option<u64>,
    /// Link quality of the last message's last hop (not persisted).
    #[serde(skip)]
    pub last_lqi: Option<u8>,
    /// Signal strength of the last message's last hop (not persisted).
    #[serde(skip)]
    pub last_rssi: Option<i8>,
}

impl Device {
    /// A device that just joined.
    pub fn new(ieee: Ieee, nwk: NodeId, now_unix: u64) -> Device {
        Device {
            ieee,
            nwk,
            name: None,
            node_descriptor: None,
            endpoints: Vec::new(),
            manufacturer: None,
            model: None,
            power_source: None,
            sw_build_id: None,
            date_code: None,
            app_version: None,
            interview: Interview::default(),
            reporting: Vec::new(),
            ias_zone_type: None,
            ias_zone_id: None,
            unique_link_key: false,
            parent: None,
            joined_unix: now_unix,
            last_seen_unix: None,
            last_lqi: None,
            last_rssi: None,
        }
    }

    /// The device's receiver is off when idle (a sleepy end device), so
    /// requests wait at its parent until it polls.
    pub fn is_sleepy(&self) -> bool {
        self.node_descriptor
            .as_ref()
            .is_some_and(|d| !d.rx_on_when_idle())
    }

    /// The endpoint that has a cluster as a server, if any.
    pub fn endpoint_with(&self, cluster: u16) -> Option<&SimpleDescriptor> {
        self.endpoints
            .iter()
            .find(|e| e.input_clusters.contains(&cluster))
    }

    /// The application profile of an endpoint (Home Automation if unknown).
    pub fn profile_of(&self, endpoint: u8) -> u16 {
        self.endpoints
            .iter()
            .find(|e| e.endpoint == endpoint)
            .map(|e| e.profile_id)
            .unwrap_or(zigbee_zcl::PROFILE_HOME_AUTOMATION)
    }
}
