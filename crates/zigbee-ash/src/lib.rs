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

pub mod conn;
pub mod frame;
#[cfg(feature = "tokio")]
pub mod transport;

pub use conn::{Config, Connection, DownReason, Event, Role, SendError};
pub use frame::{Decoder, Frame, FrameError};
#[cfg(feature = "tokio")]
pub use transport::{Link, LinkError, LinkEvent};
