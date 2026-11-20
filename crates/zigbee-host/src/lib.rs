#![doc = include_str!("../README.md")]

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
#[cfg(feature = "host")]
pub mod ota;
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
    Adapter, AdapterEvent, AdapterInfo, Beacon, JoinPolicy, NetworkInfo, NetworkSettings,
    StartMode, StartOutcome,
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
pub use host::{FrameLink, Host, HostConfig, HostEvent, OtaOffer, RawZcl, HOST_ENDPOINT};
pub use probe::{NcpIdentity, NcpKind, ProbeError};
#[cfg(feature = "host")]
pub use znp_adapter::ZnpAdapter;

// The repository README's Rust examples run as doctests, so they cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct ReadmeDoctests;
