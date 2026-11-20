#![doc = include_str!("../README.md")]

#![deny(missing_docs)]

#[cfg(feature = "client")]
pub mod client;
pub mod frame;
pub mod ids;
pub mod message;

pub use frame::{Decoder, Frame, FrameError};
pub use message::{Address, ApsConfirm, ApsIndication, ApsRequest, DecodeError, Request, Response};
