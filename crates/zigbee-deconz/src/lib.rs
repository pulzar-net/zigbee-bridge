//! dresden elektronik's deCONZ serial protocol, which ConBee, ConBee II,
//! ConBee III, RaspBee and RaspBee II coordinator firmware speaks.
//!
//! - [`frame`]: SLIP framing, the frame header and checksum.
//! - [`message`]: requests and responses, typed in both directions.
//! - [`client`] (feature `client`): an async client.
//!
//! Written from dresden elektronik's public serial protocol documentation;
//! no code was copied from any other implementation.

#![deny(missing_docs)]

#[cfg(feature = "client")]
pub mod client;
pub mod frame;
pub mod ids;
pub mod message;

pub use frame::{Decoder, Frame, FrameError};
pub use message::{Address, ApsConfirm, ApsIndication, ApsRequest, DecodeError, Request, Response};
