//! An EZSP network co-processor simulator, for testing Zigbee hosts without
//! hardware.
//!
//! [`Simulator`] speaks ASH and EZSP (protocol 13, as EmberZNet 7.4 does, or
//! 14 and later) over any byte stream, keeps a network as an NCP keeps its
//! tokens (keys, PAN ids, frame counter, key table, a written IEEE address),
//! and runs simulated [`SimDevice`]s that join when the network opens,
//! answer ZDO and ZCL requests, keep bindings and reporting configuration,
//! and report attributes the way they were configured to.
//!
//! The `zigbee-ncp-sim` binary serves it over TCP, so a bridge can be pointed
//! at `tcp://127.0.0.1:<port>`.

pub mod device;
pub mod ncp;

pub use device::{Outgoing, SimDevice, SimEndpoint};
pub use ncp::{SimConfig, SimError, SimNetwork, SimSnapshot, Simulator};
