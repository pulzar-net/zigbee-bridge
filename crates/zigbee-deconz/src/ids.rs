//! Command ids, parameters, states and statuses.

/// Command ids.
pub mod command {
    /// APS data confirm (read one).
    pub const APS_DATA_CONFIRM: u8 = 0x04;
    /// Device state.
    pub const DEVICE_STATE: u8 = 0x07;
    /// Change the network state (offline or connected).
    pub const CHANGE_NETWORK_STATE: u8 = 0x08;
    /// Read a parameter.
    pub const READ_PARAMETER: u8 = 0x0A;
    /// Write a parameter.
    pub const WRITE_PARAMETER: u8 = 0x0B;
    /// Firmware version.
    pub const VERSION: u8 = 0x0D;
    /// The device state changed (unsolicited).
    pub const DEVICE_STATE_CHANGED: u8 = 0x0E;
    /// Send an APS frame.
    pub const APS_DATA_REQUEST: u8 = 0x12;
    /// APS data indication (read one received frame).
    pub const APS_DATA_INDICATION: u8 = 0x17;
    /// A Green Power frame (unsolicited).
    pub const ZIGBEE_GREEN_POWER: u8 = 0x19;
    /// A child polled (unsolicited).
    pub const MAC_POLL: u8 = 0x1C;
}

/// Network parameters for `READ_PARAMETER` and `WRITE_PARAMETER`.
pub mod param {
    /// The coordinator's IEEE address (u64).
    pub const MAC_ADDRESS: u8 = 0x01;
    /// PAN id (u16).
    pub const NWK_PANID: u8 = 0x05;
    /// Network address (u16).
    pub const NWK_ADDRESS: u8 = 0x07;
    /// The running network's extended PAN id (u64).
    pub const NWK_EXTENDED_PANID: u8 = 0x08;
    /// Act as coordinator (u8).
    pub const APS_DESIGNED_COORDINATOR: u8 = 0x09;
    /// Channels (u32).
    pub const CHANNEL_MASK: u8 = 0x0A;
    /// The extended PAN id to form with (u64).
    pub const APS_EXTENDED_PANID: u8 = 0x0B;
    /// Trust center address (u64).
    pub const TRUST_CENTER_ADDRESS: u8 = 0x0E;
    /// Security mode (u8).
    pub const SECURITY_MODE: u8 = 0x10;
    /// Form with the configured PAN id (u8).
    pub const USE_PREDEFINED_NWK_PANID: u8 = 0x15;
    /// Network key: index (u8) then key.
    pub const NETWORK_KEY: u8 = 0x18;
    /// A link key: IEEE address (u64) then key.
    pub const LINK_KEY: u8 = 0x19;
    /// Current channel (u8).
    pub const CURRENT_CHANNEL: u8 = 0x1C;
    /// Permit joining, seconds (u8).
    pub const PERMIT_JOIN: u8 = 0x21;
    /// Serial protocol version (u16).
    pub const PROTOCOL_VERSION: u8 = 0x22;
    /// Network update id (u8).
    pub const NWK_UPDATE_ID: u8 = 0x24;
    /// Watchdog time to live, seconds (u32).
    pub const WATCHDOG_TTL: u8 = 0x26;
    /// Outgoing network frame counter (u32).
    pub const NWK_FRAME_COUNTER: u8 = 0x27;
    /// Pass ZDP responses to the host (u16).
    pub const APP_ZDP_RESPONSE_HANDLING: u8 = 0x28;
}

/// Network states (the low two bits of the device state).
pub mod net_state {
    /// Not on a network.
    pub const OFFLINE: u8 = 0;
    /// Joining.
    pub const JOINING: u8 = 1;
    /// Running.
    pub const CONNECTED: u8 = 2;
    /// Leaving.
    pub const LEAVING: u8 = 3;
}

/// Device state flags.
pub mod device_state {
    /// The network state's bits.
    pub const NETWORK_MASK: u8 = 0x03;
    /// An APS data confirm is waiting.
    pub const CONFIRM_PENDING: u8 = 0x04;
    /// An APS data indication is waiting.
    pub const INDICATION_PENDING: u8 = 0x08;
    /// The configuration changed.
    pub const CONFIG_CHANGED: u8 = 0x10;
    /// The firmware can take another APS data request.
    pub const REQUEST_FREE: u8 = 0x20;
}

/// Frame statuses.
pub mod status {
    /// Success.
    pub const SUCCESS: u8 = 0;
    /// Failure.
    pub const FAILURE: u8 = 1;
    /// Busy.
    pub const BUSY: u8 = 2;
    /// Timeout.
    pub const TIMEOUT: u8 = 3;
    /// Unsupported.
    pub const UNSUPPORTED: u8 = 4;
    /// Error.
    pub const ERROR: u8 = 5;
    /// No network.
    pub const NO_NETWORK: u8 = 6;
    /// Invalid value.
    pub const INVALID_VALUE: u8 = 7;
}

/// APS transmit options.
pub mod tx_option {
    /// Request an APS acknowledgement.
    pub const ACK: u8 = 0x04;
}

/// Security modes.
pub mod security_mode {
    /// The network key under the trust center link key.
    pub const TC_LINK_KEY: u8 = 3;
}

/// Platforms in byte 1 of the firmware version.
pub mod platform {
    /// ConBee, RaspBee (AVR).
    pub const AVR: u8 = 0x05;
    /// ConBee II, RaspBee II (ARM).
    pub const ARM: u8 = 0x07;
    /// ConBee III (EFR32).
    pub const EFR32: u8 = 0x09;
}
