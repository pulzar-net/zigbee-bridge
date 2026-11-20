//! A Zigbee coordinator host.
//!
//! The NCP runs the mesh; this crate runs the coordinator's application:
//!
//! - **Formation** with caller-supplied keys ([`NetworkSettings`]): resume
//!   the NCP's network when it matches, form it when the NCP has none, and
//!   refuse (never silently wipe) when the NCP runs another network.
//! - **Trust center**: Zigbee 3.0 unique link keys (hashed from a random
//!   root key), install codes ([`install_code`]), and a join policy that can
//!   require install codes or turn away pre-3.0 devices.
//! - **Interview** ([`interview`]): node and simple descriptors, the Basic
//!   cluster, then binding and reporting configuration from the cluster
//!   library's defaults and IAS zone enrollment; resumable per step.
//! - **Reports and commands** from devices as [`HostEvent`]s, with default
//!   responses and the Time cluster answered.
//! - **Device store** ([`store`]): one JSON file, written atomically and only
//!   when a device record changes, batched.
//! - **Backup and restore** ([`backup`]) in the open coordinator backup
//!   format: network key and frame counter, PAN ids, channel, coordinator
//!   IEEE address and trust center link keys, restorable onto a new
//!   coordinator of either family.
//! - **Topology** ([`topology`]) from last-hop link quality, route records
//!   and neighbor-table scans.
//! - **Probe** ([`probe`]): what NCP is on a port, read-only and bounded.
//!
//! The NCP sits behind the [`Adapter`] trait: [`EzspAdapter`] for Silicon
//! Labs EmberZNet 7.4 and 8.x, [`ZnpAdapter`] for Texas Instruments
//! Z-Stack 3, [`DeconzAdapter`] for dresden elektronik ConBee and RaspBee,
//! and [`AnyAdapter`] to choose at run time.
//!
//! Features: `host` (everything above), `probe` with `serial` alone for a
//! small detection tool, `serial` to open ports by name.

#[cfg(feature = "host")]
pub mod adapter;
#[cfg(feature = "host")]
pub mod any_adapter;
#[cfg(feature = "host")]
pub mod backup;
#[cfg(feature = "host")]
pub mod deconz_adapter;
#[cfg(feature = "host")]
pub mod device;
#[cfg(feature = "host")]
pub mod error;
#[cfg(feature = "host")]
pub mod ezsp_adapter;
#[cfg(feature = "host")]
mod host;
#[cfg(feature = "host")]
pub mod install_code;
#[cfg(feature = "host")]
pub mod interview;
#[cfg(feature = "serial")]
pub mod port;
pub mod probe;
#[cfg(feature = "host")]
pub mod store;
#[cfg(feature = "host")]
pub mod topology;
#[cfg(feature = "host")]
pub mod znp_adapter;

#[cfg(feature = "host")]
pub use adapter::{
    Adapter, AdapterEvent, AdapterInfo, JoinPolicy, NetworkInfo, NetworkSettings, StartMode,
    StartOutcome,
};
#[cfg(feature = "host")]
pub use any_adapter::AnyAdapter;
#[cfg(feature = "host")]
pub use backup::Backup;
#[cfg(feature = "host")]
pub use deconz_adapter::DeconzAdapter;
#[cfg(feature = "host")]
pub use device::{Device, InterviewStage};
#[cfg(feature = "host")]
pub use error::HostError;
#[cfg(feature = "host")]
pub use ezsp_adapter::{random_network, EzspAdapter};
#[cfg(feature = "host")]
pub use host::{FrameLink, Host, HostConfig, HostEvent, RawZcl, HOST_ENDPOINT};
pub use probe::{NcpIdentity, NcpKind, ProbeError};
#[cfg(feature = "host")]
pub use znp_adapter::ZnpAdapter;

// The repository README's Rust examples run as doctests, so they cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct ReadmeDoctests;
