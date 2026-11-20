#![doc = include_str!("../README.md")]
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
