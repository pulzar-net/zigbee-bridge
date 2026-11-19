//! EZSP, the EmberZNet Serial Protocol between a host and a Silicon Labs
//! Zigbee network co-processor (Silicon Labs UG100), for EmberZNet 7.4 and
//! 8.x (protocol versions 13 to 19).
//!
//! - [`frame`]: the legacy and extended frame headers.
//! - [`command`]: typed commands and their responses, encoding and decoding
//!   in both directions, with the layouts that changed at protocol 14.
//! - [`callback`]: typed callbacks.
//! - [`types`]: EZSP structures and [`types::Status`], which spans the 8-bit
//!   `EmberStatus` (protocol 13) and 32-bit `sl_status_t` (14+) code spaces.
//! - [`ids`]: frame, configuration, policy and value ids.
//! - [`client`] (feature `client`): an async client over an ASH link that
//!   negotiates the version, serializes commands and delivers callbacks.
//!
//! Every parser is bounds-checked and allocation-bounded by its input.
#![cfg_attr(
    not(test),
    deny(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )
)]

pub mod callback;
#[cfg(feature = "client")]
pub mod client;
pub mod command;
pub mod frame;
pub mod ids;
pub mod types;

pub use callback::Callback;
#[cfg(feature = "client")]
pub use client::{ClientConfig, Ezsp, EzspError, EzspEvent, NcpInfo};
pub use command::{Command, DecodeError, Response};
pub use types::{ApsFrame, ProtocolVersion, RxPacketInfo, Status};
