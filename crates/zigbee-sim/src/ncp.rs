//! The simulator: one simulated network behind the NCP family of your
//! choice.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use zigbee_types::{Ieee, Key, NodeId};
use zigbee_zcl::Value;

use crate::device::{DeviceState, Outgoing, SimDevice};
use crate::ezsp::EzspFront;
use crate::mesh::{Mesh, SimNetwork};
use crate::znp::ZnpFront;

/// Which coordinator family the simulator plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimNcp {
    /// Silicon Labs EmberZNet over EZSP and ASH. `protocol` 13 is EmberZNet
    /// 7.4; 14 to 19 are 8.x.
    Ezsp {
        /// EZSP protocol version.
        protocol: u8,
    },
    /// Texas Instruments Z-Stack over ZNP.
    Znp {
        /// Product: 1 for Z-Stack 3.x.0 (CC2652), 2 for 3.0.x (CC2531).
        product: u8,
    },
}

impl Default for SimNcp {
    fn default() -> Self {
        SimNcp::Ezsp { protocol: 13 }
    }
}

/// Simulator settings.
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// The coordinator family and version.
    pub ncp: SimNcp,
    /// The coordinator's factory IEEE address.
    pub ieee: Ieee,
    /// The simulated devices.
    pub devices: Vec<SimDevice>,
    /// Delay before a device in pairing mode joins an open network.
    pub join_delay: Duration,
    /// Delay of every device reply.
    pub reply_delay: Duration,
    /// Key table entries (EZSP).
    pub key_table_size: u16,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            ncp: SimNcp::default(),
            ieee: Ieee(0x00_0D_6F_FF_FE_00_00_01),
            devices: Vec::new(),
            join_delay: Duration::from_millis(200),
            reply_delay: Duration::from_millis(20),
            key_table_size: 12,
        }
    }
}

/// A point-in-time view for tests.
#[derive(Debug, Clone)]
pub struct SimSnapshot {
    /// The stored network, if any.
    pub network: Option<SimNetwork>,
    /// Joined devices and their network addresses.
    pub joined: Vec<(Ieee, NodeId)>,
    /// Bindings: (device, endpoint, cluster).
    pub bindings: Vec<(Ieee, u8, u16)>,
    /// Reporting configurations: (device, endpoint, cluster, attribute).
    pub reporting: Vec<(Ieee, u8, u16, u16)>,
    /// Joining is open.
    pub permit_join: bool,
    /// The IEEE address the NCP reports (factory or written).
    pub ieee: Ieee,
    /// Link keys the NCP holds for individual devices.
    pub key_table: Vec<(Ieee, Key)>,
    /// IAS-enrolled devices.
    pub ias_enrolled: Vec<Ieee>,
}

pub(crate) enum Front {
    Ezsp(EzspFront),
    Znp(ZnpFront),
}

impl Front {
    pub fn ezsp(&mut self) -> Option<&mut EzspFront> {
        match self {
            Front::Ezsp(f) => Some(f),
            _ => None,
        }
    }

    pub fn znp(&mut self) -> Option<&mut ZnpFront> {
        match self {
            Front::Znp(f) => Some(f),
            _ => None,
        }
    }

    fn link_keys(&self) -> Vec<(Ieee, Key)> {
        match self {
            Front::Ezsp(f) => f.key_table.values().copied().collect(),
            Front::Znp(f) => f.link_keys.iter().map(|(i, k)| (*i, *k)).collect(),
        }
    }
}

pub(crate) struct SimState {
    pub mesh: Mesh,
    pub front: Front,
}

/// An NCP simulator. State (the stored network, joined devices, bindings)
/// survives host reconnects, as an NCP's non-volatile memory survives
/// resets.
#[derive(Clone)]
pub struct Simulator {
    pub(crate) state: Arc<Mutex<SimState>>,
}

impl std::fmt::Debug for Simulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Simulator").finish_non_exhaustive()
    }
}

/// The simulator could not run.
#[derive(Debug, thiserror::Error)]
pub enum SimError {
    /// The host's link ended.
    #[error("host link closed: {0}")]
    Closed(String),
}

pub(crate) fn lock(m: &Mutex<SimState>) -> MutexGuard<'_, SimState> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Simulator {
    /// A simulator with `config`'s devices, all powered on.
    pub fn new(config: SimConfig) -> Simulator {
        let mesh = Mesh::new(
            config.ieee,
            &config.devices,
            config.join_delay,
            config.reply_delay,
        );
        let front = match config.ncp {
            SimNcp::Ezsp { protocol } => {
                Front::Ezsp(EzspFront::new(protocol, config.key_table_size))
            }
            SimNcp::Znp { product } => Front::Znp(ZnpFront::new(product)),
        };
        Simulator {
            state: Arc::new(Mutex::new(SimState { mesh, front })),
        }
    }

    /// Everything the simulator holds, for assertions.
    pub fn snapshot(&self) -> SimSnapshot {
        let s = lock(&self.state);
        let mut bindings = Vec::new();
        let mut reporting = Vec::new();
        for (ieee, j) in &s.mesh.joined {
            bindings.extend(
                j.state
                    .bindings
                    .iter()
                    .map(|b| (*ieee, b.source_endpoint, b.cluster)),
            );
            reporting.extend(
                j.state
                    .reporting
                    .keys()
                    .map(|(e, c, a)| (*ieee, *e, *c, *a)),
            );
        }
        reporting.sort_unstable();
        SimSnapshot {
            network: s.mesh.network.clone(),
            joined: s.mesh.joined.iter().map(|(i, j)| (*i, j.nwk)).collect(),
            bindings,
            reporting,
            permit_join: s.mesh.permit_open(),
            ieee: s.mesh.ieee(),
            key_table: s.front.link_keys(),
            ias_enrolled: s
                .mesh
                .joined
                .iter()
                .filter(|(_, j)| j.state.ias_enrolled)
                .map(|(i, _)| *i)
                .collect(),
        }
    }

    /// Set a device attribute, reporting it if the device was configured to.
    /// Returns how many reports were sent.
    pub fn set_attribute(
        &self,
        ieee: Ieee,
        endpoint: u8,
        cluster: u16,
        attribute: u16,
        value: Value,
    ) -> usize {
        let mut s = lock(&self.state);
        let mesh = &mut s.mesh;
        let Some(mut device) = mesh.devices.get(&ieee).cloned() else {
            return 0;
        };
        let reports = match mesh.joined.get_mut(&ieee) {
            Some(j) => device.set_attribute(&mut j.state, endpoint, cluster, attribute, value),
            None => {
                device.set_attribute(
                    &mut DeviceState::default(),
                    endpoint,
                    cluster,
                    attribute,
                    value,
                );
                Vec::new()
            }
        };
        mesh.devices.insert(ieee, device);
        let n = reports.len();
        for r in reports {
            mesh.deliver(ieee, r, Duration::ZERO);
        }
        n
    }

    /// A device sends a cluster command frame (a button press, an alarm).
    pub fn send_from(&self, ieee: Ieee, out: Outgoing) {
        lock(&self.state).mesh.deliver(ieee, out, Duration::ZERO);
    }

    /// Add a device (it joins the next time the network opens).
    pub fn add_device(&self, device: SimDevice) {
        let mut s = lock(&self.state);
        let ieee = device.ieee;
        s.mesh.devices.insert(ieee, device);
        if s.mesh.permit_open() {
            let delay = s.mesh.join_delay;
            s.mesh.schedule(delay, crate::mesh::Scheduled::Join(ieee));
        }
    }

    /// A device leaves the network by itself.
    pub fn leave(&self, ieee: Ieee) {
        lock(&self.state).mesh.leave_device(ieee);
    }

    /// Serve one host connection over `io` until it ends.
    pub async fn serve<T>(&self, io: T) -> Result<(), SimError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let is_ezsp = matches!(lock(&self.state).front, Front::Ezsp(_));
        if is_ezsp {
            crate::ezsp::serve(self, io).await
        } else {
            crate::znp::serve(self, io).await
        }
    }
}
