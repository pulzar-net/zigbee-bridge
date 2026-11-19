//! Host errors.

use zigbee_types::{Ieee, NodeId};

/// A host operation failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum HostError {
    /// The NCP failed or rejected a command.
    #[error("NCP: {0}")]
    Ncp(String),
    /// The NCP runs a different network than configured. Nothing was
    /// changed; restore a backup or allow replacing it.
    #[error("the coordinator runs another network ({found}); refusing to replace it")]
    NetworkMismatch {
        /// What the NCP has.
        found: String,
    },
    /// A restore needs the coordinator's IEEE address and this chip cannot
    /// take it.
    #[error("cannot restore: {0}")]
    RestoreRefused(String),
    /// The coordinator's IEEE address was written; the NCP must be power
    /// cycled (or the host restarted) before the restore can finish.
    #[error("coordinator IEEE address written; restart to finish the restore")]
    RestartRequired,
    /// No such device.
    #[error("unknown device {0}")]
    UnknownDevice(Ieee),
    /// The device did not answer in time.
    #[error("no response from {0:#06x}")]
    Timeout(NodeId),
    /// The NCP could not deliver the frame.
    #[error("delivery to {0:#06x} failed")]
    DeliveryFailed(NodeId),
    /// The device answered with a failure status.
    #[error("{what} failed with status {status:#04x}")]
    Status {
        /// The request.
        what: String,
        /// The status.
        status: u8,
    },
    /// A frame did not decode.
    #[error("bad frame: {0}")]
    BadFrame(String),
    /// A request was not valid.
    #[error("{0}")]
    Invalid(String),
    /// Store or backup file I/O.
    #[error("{0}")]
    Io(String),
    /// The host has stopped.
    #[error("host stopped")]
    Stopped,
}

impl From<zigbee_ezsp::EzspError> for HostError {
    fn from(e: zigbee_ezsp::EzspError) -> Self {
        HostError::Ncp(e.to_string())
    }
}
