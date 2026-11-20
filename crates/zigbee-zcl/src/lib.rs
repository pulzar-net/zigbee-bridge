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

pub mod defs;
pub mod foundation;
pub mod frame;
pub mod status;
pub mod types;
pub mod value;

pub use defs::{
    attr, cluster, cluster_by_name, cluster_id, command, response, ClusterDef, CLUSTERS,
};
pub use foundation::Global;
pub use frame::{Direction, Frame, FrameType, Header};
pub use types::DataType;
pub use value::Value;

/// The Home Automation / Zigbee 3.0 application profile.
pub const PROFILE_HOME_AUTOMATION: u16 = 0x0104;
/// The Zigbee Light Link profile.
pub const PROFILE_LIGHT_LINK: u16 = 0xC05E;
/// The Smart Energy profile.
pub const PROFILE_SMART_ENERGY: u16 = 0x0109;
