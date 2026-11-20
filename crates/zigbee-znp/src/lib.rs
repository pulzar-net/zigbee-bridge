//! Texas Instruments Z-Stack ZNP: the Monitor and Test (MT) serial protocol
//! that Z-Stack coordinator firmware speaks, for Z-Stack 3.0.x (CC2530,
//! CC2531) and 3.x.0 (CC13x2, CC26x2, CC2652).
//!
//! - [`frame`]: MT frames and a streaming decoder.
//! - [`message`]: the requests a host sends and the indications the NCP
//!   sends, typed in both directions.
//! - [`client`] (feature `client`): an async client.
//!
//! Written from TI's public MT API documentation; no code was copied from
//! any other implementation.

#![deny(missing_docs)]

#[cfg(feature = "client")]
pub mod client;
pub mod frame;
pub mod ids;
pub mod message;

pub use frame::{Decoded, Decoder, Frame, FrameError, Kind};
pub use message::{DecodeError, IncomingMsg, Indication, Request};
