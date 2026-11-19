//! ASHv2, the Asynchronous Serial Host protocol that carries EZSP between a
//! host and a Silicon Labs EmberZNet network co-processor (Silicon Labs
//! UG101).
//!
//! - [`frame`]: frame encoding and decoding, CRC-CCITT, byte stuffing, data
//!   randomization, and a bounded stream [`frame::Decoder`].
//! - [`conn`]: the link as a sans-IO state machine: RST/RSTACK, frame
//!   numbering, piggybacked and explicit ACKs, NAK on loss or corruption,
//!   go-back-N retransmission, a sliding window and UG101's adaptive ACK
//!   timer. It serves both ends of the link.
//! - [`transport`] (feature `tokio`): a task that runs a connection over any
//!   `AsyncRead + AsyncWrite`, such as a serial port or a TCP socket.
#![cfg_attr(
    not(test),
    deny(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )
)]

pub mod conn;
pub mod frame;
#[cfg(feature = "tokio")]
pub mod transport;

pub use conn::{Config, Connection, DownReason, Event, Role, SendError};
pub use frame::{Decoder, Frame, FrameError};
#[cfg(feature = "tokio")]
pub use transport::{Link, LinkError, LinkEvent};
