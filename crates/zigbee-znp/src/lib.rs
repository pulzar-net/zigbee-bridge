#![doc = include_str!("../README.md")]

#![deny(missing_docs)]

#[cfg(feature = "client")]
pub mod client;
pub mod frame;
pub mod ids;
pub mod message;

pub use frame::{Decoded, Decoder, Frame, FrameError, Kind};
pub use message::{Beacon, DecodeError, IncomingMsg, Indication, Request};
