//! EZSP frame ids and the ids of configuration values, policies, decisions,
//! values and manufacturing tokens, as Silicon Labs documents them (UG100
//! and the EmberZNet `ezsp-enum.h`).

/// Frame ids (commands, their responses, and callbacks).
pub mod frame {
    #![allow(missing_docs)]
    pub const VERSION: u16 = 0x0000;
    pub const ADD_ENDPOINT: u16 = 0x0002;
    pub const NOP: u16 = 0x0005;
    pub const NO_CALLBACKS: u16 = 0x0007;
    pub const GET_MFG_TOKEN: u16 = 0x000B;
    pub const SET_MFG_TOKEN: u16 = 0x000C;
    pub const STACK_TOKEN_CHANGED_HANDLER: u16 = 0x000D;
    pub const SET_CONCENTRATOR: u16 = 0x0010;
    pub const SET_MANUFACTURER_CODE: u16 = 0x0015;
    pub const NETWORK_INIT: u16 = 0x0017;
    pub const NETWORK_STATE: u16 = 0x0018;
    pub const STACK_STATUS_HANDLER: u16 = 0x0019;
    pub const START_SCAN: u16 = 0x001A;
    pub const NETWORK_FOUND_HANDLER: u16 = 0x001B;
    pub const SCAN_COMPLETE_HANDLER: u16 = 0x001C;
    pub const FORM_NETWORK: u16 = 0x001E;
    pub const LEAVE_NETWORK: u16 = 0x0020;
    pub const PERMIT_JOINING: u16 = 0x0022;
    pub const CHILD_JOIN_HANDLER: u16 = 0x0023;
    pub const TRUST_CENTER_JOIN_HANDLER: u16 = 0x0024;
    pub const GET_EUI64: u16 = 0x0026;
    pub const GET_NODE_ID: u16 = 0x0027;
    pub const GET_NETWORK_PARAMETERS: u16 = 0x0028;
    pub const SEND_UNICAST: u16 = 0x0034;
    pub const SEND_BROADCAST: u16 = 0x0036;
    pub const MESSAGE_SENT_HANDLER: u16 = 0x003F;
    pub const INCOMING_MESSAGE_HANDLER: u16 = 0x0045;
    pub const ENERGY_SCAN_RESULT_HANDLER: u16 = 0x0048;
    pub const GET_CONFIGURATION_VALUE: u16 = 0x0052;
    pub const SET_CONFIGURATION_VALUE: u16 = 0x0053;
    pub const SET_POLICY: u16 = 0x0055;
    pub const INVALID_COMMAND: u16 = 0x0058;
    pub const INCOMING_ROUTE_RECORD_HANDLER: u16 = 0x0059;
    pub const SET_SOURCE_ROUTE_DISCOVERY_MODE: u16 = 0x005A;
    pub const LOOKUP_EUI64_BY_NODE_ID: u16 = 0x0061;
    pub const SET_INITIAL_SECURITY_STATE: u16 = 0x0068;
    /// Before protocol 13 only.
    pub const GET_KEY: u16 = 0x006A;
    pub const CLEAR_TRANSIENT_LINK_KEYS: u16 = 0x006B;
    /// Before protocol 13 only.
    pub const GET_KEY_TABLE_ENTRY: u16 = 0x0071;
    /// Before protocol 13 only.
    pub const SET_KEY_TABLE_ENTRY: u16 = 0x0072;
    pub const ERASE_KEY_TABLE_ENTRY: u16 = 0x0076;
    pub const ID_CONFLICT_HANDLER: u16 = 0x007C;
    pub const INCOMING_MANY_TO_ONE_ROUTE_REQUEST_HANDLER: u16 = 0x007D;
    pub const INCOMING_ROUTE_ERROR_HANDLER: u16 = 0x0080;
    pub const ECHO: u16 = 0x0081;
    pub const SET_RADIO_POWER: u16 = 0x0099;
    pub const ZIGBEE_KEY_ESTABLISHMENT_HANDLER: u16 = 0x009B;
    pub const GET_VALUE: u16 = 0x00AA;
    pub const SET_VALUE: u16 = 0x00AB;
    /// Before protocol 13 only.
    pub const ADD_TRANSIENT_LINK_KEY: u16 = 0x00AF;
    pub const CLEAR_KEY_TABLE: u16 = 0x00B1;
    pub const INCOMING_NETWORK_STATUS_HANDLER: u16 = 0x00C4;
    pub const IMPORT_LINK_KEY: u16 = 0x010E;
    pub const EXPORT_LINK_KEY_BY_INDEX: u16 = 0x010F;
    pub const IMPORT_TRANSIENT_KEY: u16 = 0x0111;
    pub const EXPORT_KEY: u16 = 0x0114;
    pub const GET_NETWORK_KEY_INFO: u16 = 0x0116;
}

/// Configuration ids (`setConfigurationValue`), set before the network starts.
pub mod config {
    #![allow(missing_docs)]
    pub const PACKET_BUFFER_HEAP_SIZE: u8 = 0x01;
    pub const NEIGHBOR_TABLE_SIZE: u8 = 0x02;
    pub const APS_UNICAST_MESSAGE_COUNT: u8 = 0x03;
    pub const BINDING_TABLE_SIZE: u8 = 0x04;
    pub const ADDRESS_TABLE_SIZE: u8 = 0x05;
    pub const MULTICAST_TABLE_SIZE: u8 = 0x06;
    pub const ROUTE_TABLE_SIZE: u8 = 0x07;
    pub const DISCOVERY_TABLE_SIZE: u8 = 0x08;
    pub const STACK_PROFILE: u8 = 0x0C;
    pub const SECURITY_LEVEL: u8 = 0x0D;
    pub const MAX_HOPS: u8 = 0x10;
    pub const MAX_END_DEVICE_CHILDREN: u8 = 0x11;
    pub const INDIRECT_TRANSMISSION_TIMEOUT: u8 = 0x12;
    pub const END_DEVICE_POLL_TIMEOUT: u8 = 0x13;
    pub const TRUST_CENTER_ADDRESS_CACHE_SIZE: u8 = 0x19;
    pub const SOURCE_ROUTE_TABLE_SIZE: u8 = 0x1A;
    pub const KEY_TABLE_SIZE: u8 = 0x1E;
    pub const SUPPORTED_NETWORKS: u8 = 0x2D;
    pub const TRANSIENT_KEY_TIMEOUT_S: u8 = 0x36;
}

/// Policy ids (`setPolicy`).
pub mod policy {
    #![allow(missing_docs)]
    pub const TRUST_CENTER: u8 = 0x00;
    pub const BINDING_MODIFICATION: u8 = 0x01;
    pub const UNICAST_REPLIES: u8 = 0x02;
    pub const POLL_HANDLER: u8 = 0x03;
    pub const MESSAGE_CONTENTS_IN_CALLBACK: u8 = 0x04;
    pub const TC_KEY_REQUEST: u8 = 0x05;
    pub const APP_KEY_REQUEST: u8 = 0x06;
}

/// Decision ids for the policies above.
pub mod decision {
    #![allow(missing_docs)]
    pub const CHECK_BINDING_MODIFICATIONS_ARE_VALID_ENDPOINT_CLUSTERS: u8 = 0x12;
    pub const HOST_WILL_NOT_SUPPLY_REPLY: u8 = 0x20;
    pub const MESSAGE_TAG_ONLY_IN_CALLBACK: u8 = 0x40;
    pub const DENY_TC_KEY_REQUESTS: u8 = 0x50;
    pub const ALLOW_TC_KEY_REQUESTS_AND_SEND_CURRENT_KEY: u8 = 0x51;
    pub const ALLOW_TC_KEY_REQUEST_AND_GENERATE_NEW_KEY: u8 = 0x52;
    pub const DENY_APP_KEY_REQUESTS: u8 = 0x60;
    pub const ALLOW_APP_KEY_REQUESTS: u8 = 0x61;
}

/// Bits of the trust center policy decision.
pub mod tc_decision {
    #![allow(missing_docs)]
    pub const ALLOW_JOINS: u8 = 0x01;
    pub const ALLOW_UNSECURED_REJOINS: u8 = 0x02;
    pub const SEND_KEY_IN_CLEAR: u8 = 0x04;
    pub const IGNORE_UNSECURED_REJOINS: u8 = 0x08;
    pub const JOINS_USE_INSTALL_CODE_KEY: u8 = 0x10;
    pub const DEFER_JOINS: u8 = 0x20;
}

/// Value ids (`getValue` / `setValue`).
pub mod value {
    #![allow(missing_docs)]
    pub const STACK_TOKEN_WRITING: u8 = 0x07;
    pub const EXTENDED_SECURITY_BITMASK: u8 = 0x0A;
    pub const VERSION_INFO: u8 = 0x11;
    pub const NWK_FRAME_COUNTER: u8 = 0x23;
    pub const APS_FRAME_COUNTER: u8 = 0x24;
}

/// Manufacturing token ids (`getMfgToken` / `setMfgToken`).
pub mod mfg_token {
    #![allow(missing_docs)]
    pub const STRING: u8 = 0x01;
    pub const BOARD_NAME: u8 = 0x02;
    pub const MANUF_ID: u8 = 0x03;
    pub const CUSTOM_EUI_64: u8 = 0x0C;
}

/// Bits of `EmberInitialSecurityState.bitmask`.
pub mod security {
    #![allow(missing_docs)]
    pub const TRUST_CENTER_GLOBAL_LINK_KEY: u16 = 0x0004;
    pub const TRUST_CENTER_USES_HASHED_LINK_KEY: u16 = 0x0084;
    pub const HAVE_PRECONFIGURED_KEY: u16 = 0x0100;
    pub const HAVE_NETWORK_KEY: u16 = 0x0200;
    pub const REQUIRE_ENCRYPTED_KEY: u16 = 0x0800;
    pub const NO_FRAME_COUNTER_RESET: u16 = 0x1000;
}

/// Bits of the extended security bitmask (`EXTENDED_SECURITY_BITMASK` value).
pub mod ext_security {
    #![allow(missing_docs)]
    pub const JOINER_GLOBAL_LINK_KEY: u16 = 0x0010;
    pub const NWK_LEAVE_REQUEST_NOT_ALLOWED: u16 = 0x0100;
}

/// `EmberNetworkStatus`.
pub mod network_status {
    #![allow(missing_docs)]
    pub const NO_NETWORK: u8 = 0;
    pub const JOINING_NETWORK: u8 = 1;
    pub const JOINED_NETWORK: u8 = 2;
    pub const JOINED_NETWORK_NO_PARENT: u8 = 3;
    pub const LEAVING_NETWORK: u8 = 6;
}

/// `EmberNodeType`.
pub mod node_type {
    #![allow(missing_docs)]
    pub const UNKNOWN: u8 = 0;
    pub const COORDINATOR: u8 = 1;
    pub const ROUTER: u8 = 2;
    pub const END_DEVICE: u8 = 3;
    pub const SLEEPY_END_DEVICE: u8 = 4;
}

/// `EmberOutgoingMessageType`.
pub mod outgoing {
    #![allow(missing_docs)]
    pub const DIRECT: u8 = 0;
    pub const VIA_ADDRESS_TABLE: u8 = 1;
    pub const VIA_BINDING: u8 = 2;
    pub const MULTICAST: u8 = 3;
    pub const BROADCAST: u8 = 6;
}

/// `EmberIncomingMessageType`.
pub mod incoming {
    #![allow(missing_docs)]
    pub const UNICAST: u8 = 0;
    pub const UNICAST_REPLY: u8 = 1;
    pub const MULTICAST: u8 = 2;
    pub const MULTICAST_LOOPBACK: u8 = 3;
    pub const BROADCAST: u8 = 4;
    pub const BROADCAST_LOOPBACK: u8 = 5;
}

/// `EmberApsOption` bits.
pub mod aps_option {
    #![allow(missing_docs)]
    pub const NONE: u16 = 0x0000;
    pub const ENCRYPTION: u16 = 0x0020;
    pub const RETRY: u16 = 0x0040;
    pub const ENABLE_ROUTE_DISCOVERY: u16 = 0x0100;
    pub const FORCE_ROUTE_DISCOVERY: u16 = 0x0200;
    pub const SOURCE_EUI64: u16 = 0x0400;
    pub const DESTINATION_EUI64: u16 = 0x0800;
    pub const ENABLE_ADDRESS_DISCOVERY: u16 = 0x1000;
    pub const ZDO_RESPONSE_REQUIRED: u16 = 0x4000;
}

/// `EmberDeviceUpdate`, the status in `trustCenterJoinHandler`.
pub mod device_update {
    #![allow(missing_docs)]
    pub const SECURED_REJOIN: u8 = 0;
    pub const UNSECURED_JOIN: u8 = 1;
    pub const DEVICE_LEFT: u8 = 2;
    pub const UNSECURED_REJOIN: u8 = 3;
}

/// `EmberJoinDecision`.
pub mod join_decision {
    #![allow(missing_docs)]
    pub const USE_PRECONFIGURED_KEY: u8 = 0;
    pub const SEND_KEY_IN_THE_CLEAR: u8 = 1;
    pub const DENY_JOIN: u8 = 2;
    pub const NO_ACTION: u8 = 3;
    pub const ALLOW_REJOINS_ONLY: u8 = 4;
}

/// `EmberKeyStatus`, from `zigbeeKeyEstablishmentHandler`.
pub mod key_status {
    #![allow(missing_docs)]
    pub const TRUST_CENTER_LINK_KEY_ESTABLISHED: u8 = 0x03;
    pub const TC_RESPONDED_TO_KEY_REQUEST: u8 = 0x06;
    pub const TC_FAILED_TO_SEND_TC_KEY: u8 = 0x13;
    pub const VERIFY_LINK_KEY_SUCCESS: u8 = 0x65;
}

/// `sl_zb_sec_man_key_type_t`.
pub mod key_type {
    #![allow(missing_docs)]
    pub const NONE: u8 = 0;
    pub const NETWORK: u8 = 1;
    pub const TC_LINK: u8 = 2;
    pub const APP_LINK: u8 = 4;
}

/// `EmberKeyType`, for `getKey` (protocol versions before 13).
pub mod ember_key_type {
    #![allow(missing_docs)]
    pub const TRUST_CENTER_LINK: u8 = 1;
    pub const CURRENT_NETWORK: u8 = 3;
    pub const NEXT_NETWORK: u8 = 4;
    pub const APPLICATION_LINK: u8 = 5;
}

/// `EmberKeyStructBitmask`: which fields of a [`crate::types::KeyStruct`]
/// are valid.
pub mod key_struct {
    #![allow(missing_docs)]
    pub const HAS_SEQUENCE_NUMBER: u16 = 0x0001;
    pub const HAS_OUTGOING_FRAME_COUNTER: u16 = 0x0002;
    pub const HAS_INCOMING_FRAME_COUNTER: u16 = 0x0004;
    pub const HAS_PARTNER_EUI64: u16 = 0x0008;
}

/// `EmberNetworkInitBitmask`.
pub mod network_init {
    #![allow(missing_docs)]
    pub const PARENT_INFO_IN_TOKEN: u16 = 0x0001;
    pub const END_DEVICE_REJOIN_ON_REBOOT: u16 = 0x0002;
}

/// Scan types for `startScan`.
pub mod scan_type {
    #![allow(missing_docs)]
    pub const ENERGY: u8 = 0;
    pub const ACTIVE: u8 = 1;
}

/// The concentrator types for `setConcentrator`.
pub mod concentrator {
    #![allow(missing_docs)]
    pub const LOW_RAM: u16 = 0xFFF8;
    pub const HIGH_RAM: u16 = 0xFFF9;
}

/// The stack type the `version` command reports for a Zigbee mesh stack.
pub const STACK_TYPE_MESH: u8 = 0x02;
/// Zigbee PRO stack profile.
pub const STACK_PROFILE_ZIGBEE_PRO: u16 = 0x02;
/// Security level 5 (Zigbee 3.0).
pub const SECURITY_LEVEL_Z3: u16 = 0x05;
