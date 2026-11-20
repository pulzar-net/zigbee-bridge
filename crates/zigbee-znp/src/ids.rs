//! Subsystems, command ids, NV item ids and status codes.

/// MT subsystems.
pub mod subsystem {
    /// Reserved; the NCP's "unknown command" error comes from here.
    pub const RPC_ERROR: u8 = 0x00;
    /// System: reset, version, NV memory.
    pub const SYS: u8 = 0x01;
    /// Application framework: endpoints and data.
    pub const AF: u8 = 0x04;
    /// Zigbee device objects.
    pub const ZDO: u8 = 0x05;
    /// Utilities.
    pub const UTIL: u8 = 0x07;
    /// Base device behavior configuration.
    pub const APP_CNF: u8 = 0x0F;
}

/// SYS commands.
pub mod sys {
    /// Reset (AREQ).
    pub const RESET_REQ: u8 = 0x00;
    /// Ping.
    pub const PING: u8 = 0x01;
    /// Version.
    pub const VERSION: u8 = 0x02;
    /// Create an OSAL NV item.
    pub const OSAL_NV_ITEM_INIT: u8 = 0x07;
    /// Read an OSAL NV item.
    pub const OSAL_NV_READ: u8 = 0x08;
    /// Write an OSAL NV item.
    pub const OSAL_NV_WRITE: u8 = 0x09;
    /// Delete an OSAL NV item.
    pub const OSAL_NV_DELETE: u8 = 0x12;
    /// An OSAL NV item's length.
    pub const OSAL_NV_LENGTH: u8 = 0x13;
    /// Transmit power.
    pub const SET_TX_POWER: u8 = 0x14;
    /// Create an extended NV item (Z-Stack 3.x.0).
    pub const NV_CREATE: u8 = 0x30;
    /// Delete an extended NV item.
    pub const NV_DELETE: u8 = 0x31;
    /// An extended NV item's length.
    pub const NV_LENGTH: u8 = 0x32;
    /// Read an extended NV item.
    pub const NV_READ: u8 = 0x33;
    /// Write an extended NV item.
    pub const NV_WRITE: u8 = 0x34;
    /// Reset indication (AREQ).
    pub const RESET_IND: u8 = 0x80;
}

/// AF commands.
pub mod af {
    /// Register an endpoint.
    pub const REGISTER: u8 = 0x00;
    /// Send data, short addressing.
    pub const DATA_REQUEST: u8 = 0x01;
    /// Send data, any addressing and long payloads.
    pub const DATA_REQUEST_EXT: u8 = 0x02;
    /// Data confirm (AREQ).
    pub const DATA_CONFIRM: u8 = 0x80;
    /// Incoming message (AREQ).
    pub const INCOMING_MSG: u8 = 0x81;
    /// Incoming message, extended (AREQ).
    pub const INCOMING_MSG_EXT: u8 = 0x82;
}

/// ZDO commands.
pub mod zdo {
    /// Install a link key for a device.
    pub const SET_LINK_KEY: u8 = 0x23;
    /// Send a raw ZDO frame.
    pub const SEND_DATA: u8 = 0x28;
    /// Permit joining.
    pub const MGMT_PERMIT_JOIN_REQ: u8 = 0x36;
    /// Network update (channel change, energy scan).
    pub const MGMT_NWK_UPDATE_REQ: u8 = 0x37;
    /// Forward ZDO frames of a cluster to the host.
    pub const MSG_CB_REGISTER: u8 = 0x3E;
    /// Start the stack.
    pub const STARTUP_FROM_APP: u8 = 0x40;
    /// Network information.
    pub const EXT_NWK_INFO: u8 = 0x50;
    /// Device state changed (AREQ).
    pub const STATE_CHANGE_IND: u8 = 0xC0;
    /// Device announce (AREQ).
    pub const END_DEVICE_ANNCE_IND: u8 = 0xC1;
    /// Source route (AREQ).
    pub const SRC_RTG_IND: u8 = 0xC4;
    /// A device left (AREQ).
    pub const LEAVE_IND: u8 = 0xC9;
    /// The trust center admitted a device (AREQ).
    pub const TC_DEV_IND: u8 = 0xCA;
    /// Permit join state (AREQ).
    pub const PERMIT_JOIN_IND: u8 = 0xCB;
    /// A forwarded ZDO frame (AREQ).
    pub const MSG_CB_INCOMING: u8 = 0xFF;
}

/// UTIL commands.
pub mod util {
    /// Device information.
    pub const GET_DEVICE_INFO: u8 = 0x00;
}

/// APP_CNF commands.
pub mod app_cnf {
    /// Trust center policy on rejoins.
    pub const SET_ALLOWREJOIN_TC_POLICY: u8 = 0x03;
    /// Add an install code.
    pub const BDB_ADD_INSTALLCODE: u8 = 0x04;
    /// Only devices with an install code may join.
    pub const BDB_SET_JOINUSESINSTALLCODEKEY: u8 = 0x06;
    /// Start commissioning.
    pub const BDB_START_COMMISSIONING: u8 = 0x05;
    /// Commissioning channels.
    pub const BDB_SET_CHANNEL: u8 = 0x08;
    /// Require a trust center link key exchange from joiners.
    pub const BDB_SET_TC_REQUIRE_KEY_EXCHANGE: u8 = 0x09;
    /// Commissioning progress (AREQ).
    pub const BDB_COMMISSIONING_NOTIFICATION: u8 = 0x80;
}

/// Commissioning modes for `BDB_START_COMMISSIONING`.
pub mod commissioning {
    /// Network steering (open for joining).
    pub const STEERING: u8 = 0x02;
    /// Network formation.
    pub const FORMATION: u8 = 0x04;
}

/// Device states (`ZDO_STATE_CHANGE_IND`).
pub mod state {
    /// Initialized, not started.
    pub const HOLD: u8 = 0x00;
    /// Starting as coordinator.
    pub const COORD_STARTING: u8 = 0x08;
    /// Running as coordinator.
    pub const ZB_COORD: u8 = 0x09;
}

/// Z-Stack product ids from `SYS_VERSION`.
pub mod product {
    /// Z-Stack Home 1.2 (not Zigbee 3.0).
    pub const ZSTACK_12: u8 = 0;
    /// Z-Stack 3.x.0 (CC13x2, CC26x2, CC2652).
    pub const ZSTACK_3X0: u8 = 1;
    /// Z-Stack 3.0.x (CC2530, CC2531).
    pub const ZSTACK_30X: u8 = 2;
}

/// Status codes.
pub mod status {
    /// Success.
    pub const SUCCESS: u8 = 0x00;
    /// Failure.
    pub const FAILURE: u8 = 0x01;
    /// An NV item was created.
    pub const NV_ITEM_UNINIT: u8 = 0x09;
    /// NV operation failed.
    pub const NV_OPER_FAILED: u8 = 0x0A;
    /// No such NV item.
    pub const NV_BAD_ITEM_LEN: u8 = 0x0C;
    /// MAC: no acknowledgement.
    pub const MAC_NO_ACK: u8 = 0xE9;
    /// APS: no acknowledgement.
    pub const APS_NO_ACK: u8 = 0xA7;
    /// NWK: no route.
    pub const NWK_NO_ROUTE: u8 = 0xCD;
}

/// AF transmit options.
pub mod af_option {
    /// Ask for an APS acknowledgement.
    pub const ACK_REQUEST: u8 = 0x10;
    /// Discover a route if needed.
    pub const DISCOVER_ROUTE: u8 = 0x20;
}

/// Address modes in `AF_DATA_REQUEST_EXT` and `ZDO_MGMT_*`.
pub mod addr_mode {
    /// Group.
    pub const GROUP: u8 = 0x01;
    /// 16-bit network address.
    pub const SHORT: u8 = 0x02;
    /// 64-bit IEEE address.
    pub const IEEE: u8 = 0x03;
    /// Broadcast.
    pub const BROADCAST: u8 = 0x0F;
}

/// Legacy OSAL NV item ids (every Z-Stack 3 build).
pub mod nv {
    /// The coordinator's IEEE address; read at start-up.
    pub const EXTADDR: u16 = 0x0001;
    /// Start-up options: clear state and configuration on the next reset.
    pub const STARTUP_OPTION: u16 = 0x0003;
    /// Network information base.
    pub const NIB: u16 = 0x0021;
    /// Extended PAN id.
    pub const EXTENDED_PAN_ID: u16 = 0x002D;
    /// Active network key and its sequence number.
    pub const NWK_ACTIVE_KEY_INFO: u16 = 0x003A;
    /// Use the configured extended PAN id.
    pub const APS_USE_EXT_PANID: u16 = 0x0047;
    /// The network key to form with.
    pub const PRECFGKEY: u16 = 0x0062;
    /// Distribute the network key in the clear (1) or under link keys (0).
    pub const PRECFGKEYS_ENABLE: u16 = 0x0063;
    /// The network security material table (Z-Stack 3.0.x), first entry.
    pub const LEGACY_NWK_SEC_MATERIAL_TABLE_START: u16 = 0x0075;
    /// PAN id.
    pub const PANID: u16 = 0x0083;
    /// Channel mask.
    pub const CHANLIST: u16 = 0x0084;
    /// Logical type: 0 coordinator.
    pub const LOGICAL_TYPE: u16 = 0x0087;
    /// Deliver ZDO responses to the host.
    pub const ZDO_DIRECT_CB: u16 = 0x008F;
    /// The trust center's link key seed (unique keys are derived from it).
    pub const TCLK_SEED: u16 = 0x0101;
}

/// Extended NV tables (Z-Stack 3.x.0), system id 1.
pub mod nv_ex {
    /// The Z-Stack system id.
    pub const SYSID_ZSTACK: u8 = 1;
    /// The network security material table.
    pub const NWK_SEC_MATERIAL_TABLE: u16 = 0x0007;
}

/// Start-up option bits.
pub mod startup_option {
    /// Clear the network configuration.
    pub const CLEAR_CONFIG: u8 = 0x01;
    /// Clear the network state.
    pub const CLEAR_STATE: u8 = 0x02;
}
