//! ZCL status codes (ZCL chapter 2.6.3).
#![allow(missing_docs)]

pub const SUCCESS: u8 = 0x00;
pub const FAILURE: u8 = 0x01;
pub const NOT_AUTHORIZED: u8 = 0x7E;
pub const MALFORMED_COMMAND: u8 = 0x80;
pub const UNSUP_COMMAND: u8 = 0x81;
pub const UNSUP_GENERAL_COMMAND: u8 = 0x82;
pub const UNSUP_MANUF_CLUSTER_COMMAND: u8 = 0x83;
pub const UNSUP_MANUF_GENERAL_COMMAND: u8 = 0x84;
pub const INVALID_FIELD: u8 = 0x85;
pub const UNSUPPORTED_ATTRIBUTE: u8 = 0x86;
pub const INVALID_VALUE: u8 = 0x87;
pub const READ_ONLY: u8 = 0x88;
pub const INSUFFICIENT_SPACE: u8 = 0x89;
pub const DUPLICATE_EXISTS: u8 = 0x8A;
pub const NOT_FOUND: u8 = 0x8B;
pub const UNREPORTABLE_ATTRIBUTE: u8 = 0x8C;
pub const INVALID_DATA_TYPE: u8 = 0x8D;
pub const INVALID_SELECTOR: u8 = 0x8E;
pub const TIMEOUT: u8 = 0x94;
pub const ABORT: u8 = 0x95;
pub const INVALID_IMAGE: u8 = 0x96;
pub const WAIT_FOR_DATA: u8 = 0x97;
pub const NO_IMAGE_AVAILABLE: u8 = 0x98;
pub const REQUIRE_MORE_IMAGE: u8 = 0x99;
pub const NOTIFICATION_PENDING: u8 = 0x9A;
pub const HARDWARE_FAILURE: u8 = 0xC0;
pub const SOFTWARE_FAILURE: u8 = 0xC1;
pub const UNSUPPORTED_CLUSTER: u8 = 0xC3;

/// A status's name, for messages.
pub fn name(status: u8) -> &'static str {
    match status {
        SUCCESS => "success",
        FAILURE => "failure",
        NOT_AUTHORIZED => "not_authorized",
        MALFORMED_COMMAND => "malformed_command",
        UNSUP_COMMAND => "unsupported_command",
        UNSUP_GENERAL_COMMAND => "unsupported_general_command",
        UNSUP_MANUF_CLUSTER_COMMAND => "unsupported_manufacturer_cluster_command",
        UNSUP_MANUF_GENERAL_COMMAND => "unsupported_manufacturer_general_command",
        INVALID_FIELD => "invalid_field",
        UNSUPPORTED_ATTRIBUTE => "unsupported_attribute",
        INVALID_VALUE => "invalid_value",
        READ_ONLY => "read_only",
        INSUFFICIENT_SPACE => "insufficient_space",
        DUPLICATE_EXISTS => "duplicate_exists",
        NOT_FOUND => "not_found",
        UNREPORTABLE_ATTRIBUTE => "unreportable_attribute",
        INVALID_DATA_TYPE => "invalid_data_type",
        INVALID_SELECTOR => "invalid_selector",
        TIMEOUT => "timeout",
        ABORT => "abort",
        INVALID_IMAGE => "invalid_image",
        WAIT_FOR_DATA => "wait_for_data",
        NO_IMAGE_AVAILABLE => "no_image_available",
        REQUIRE_MORE_IMAGE => "require_more_image",
        NOTIFICATION_PENDING => "notification_pending",
        HARDWARE_FAILURE => "hardware_failure",
        SOFTWARE_FAILURE => "software_failure",
        UNSUPPORTED_CLUSTER => "unsupported_cluster",
        _ => "unknown",
    }
}
