//! EZSP structures and the status type, with their wire encodings.

use std::fmt;

use zigbee_types::wire::{Reader, Truncated, Writer};
use zigbee_types::{ExtPanId, Ieee, Key, NodeId, PanId};

/// The oldest EZSP protocol version this crate speaks (EmberZNet 7.4).
pub const MIN_PROTOCOL_VERSION: u8 = 13;
/// The newest EZSP protocol version this crate speaks (EmberZNet 8.x).
pub const MAX_PROTOCOL_VERSION: u8 = 19;

/// A negotiated EZSP protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion(pub u8);

impl ProtocolVersion {
    /// The newest version this crate speaks, which the host asks for first.
    pub const LATEST: ProtocolVersion = ProtocolVersion(MAX_PROTOCOL_VERSION);

    /// From version 14 (EmberZNet 8.0) statuses are 32-bit `sl_status_t`
    /// and several frames changed layout; version 13 uses 8-bit
    /// `EmberStatus`.
    pub fn sl_status(self) -> bool {
        self.0 >= 14
    }

    /// True when this crate can speak the version.
    pub fn is_supported(self) -> bool {
        (MIN_PROTOCOL_VERSION..=MAX_PROTOCOL_VERSION).contains(&self.0)
    }
}

/// A status code from the NCP.
///
/// Version 13 sends 8-bit `EmberStatus` codes, version 14 and later 32-bit
/// `sl_status_t`; `legacy` says which space `raw` is in. Use the predicates
/// rather than comparing `raw`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Status {
    /// The code as sent.
    pub raw: u32,
    /// True for an 8-bit `EmberStatus` (or `EzspStatus`) code.
    pub legacy: bool,
}

macro_rules! statuses {
    ($($name:ident, $pred:ident, $sl:expr, $ember:expr;)*) => {
        impl Status {
            $(
                #[doc = concat!("`", stringify!($name), "` in the given version's code space.")]
                pub fn $name(v: ProtocolVersion) -> Status {
                    if v.sl_status() { Status { raw: $sl, legacy: false } } else { Status { raw: $ember, legacy: true } }
                }
                #[doc = concat!("True for `", stringify!($name), "`.")]
                pub fn $pred(&self) -> bool {
                    if self.legacy { self.raw == $ember } else { self.raw == $sl }
                }
            )*

            fn name(&self) -> Option<&'static str> {
                $( if self.$pred() { return Some(stringify!($name)); } )*
                None
            }
        }
    };
}

statuses! {
    ok, is_ok, 0x0000, 0x00;
    fail, is_fail, 0x0001, 0x01;
    invalid_parameter, is_invalid_parameter, 0x0021, 0x02;
    not_found_sl, is_not_found, 0x002D, 0x002D;
    not_joined, is_not_joined, 0x0017, 0x93;
    network_up, is_network_up, 0x0015, 0x90;
    network_down, is_network_down, 0x0016, 0x91;
    network_opened, is_network_opened, 0x0C18, 0x9C;
    network_closed, is_network_closed, 0x0C19, 0x9D;
    delivery_failed, is_delivery_failed, 0x0C02, 0x66;
    invalid_call, is_invalid_call, 0x0002, 0x70;
}

impl Status {
    /// Read a status in the version's width.
    pub fn read(r: &mut Reader<'_>, v: ProtocolVersion) -> Result<Status, Truncated> {
        if v.sl_status() {
            Ok(Status {
                raw: r.u32()?,
                legacy: false,
            })
        } else {
            Ok(Status {
                raw: u32::from(r.u8()?),
                legacy: true,
            })
        }
    }

    /// Read a status that is 32 bits in every version (the security
    /// manager commands).
    pub fn read_sl(r: &mut Reader<'_>) -> Result<Status, Truncated> {
        Ok(Status {
            raw: r.u32()?,
            legacy: false,
        })
    }

    /// Write in the status's own width.
    pub fn write(&self, w: &mut Vec<u8>) {
        if self.legacy {
            w.put_u8(self.raw as u8);
        } else {
            w.put_u32(self.raw);
        }
    }

    /// `ok` as a 32-bit status, for commands that always use `sl_status_t`.
    pub const SL_OK: Status = Status {
        raw: 0,
        legacy: false,
    };
    /// `not_found` as a 32-bit status.
    pub const SL_NOT_FOUND: Status = Status {
        raw: 0x002D,
        legacy: false,
    };
}

impl fmt::Debug for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.name(), self.legacy) {
            (Some(name), true) => write!(f, "{name} (ember {:#04x})", self.raw),
            (Some(name), false) => write!(f, "{name} ({:#06x})", self.raw),
            (None, true) => write!(f, "ember status {:#04x}", self.raw),
            (None, false) => write!(f, "sl status {:#06x}", self.raw),
        }
    }
}

/// `EmberApsFrame`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApsFrame {
    /// Application profile (0x0000 ZDO, 0x0104 Home Automation).
    pub profile_id: u16,
    /// Cluster id.
    pub cluster_id: u16,
    /// Source endpoint.
    pub source_endpoint: u8,
    /// Destination endpoint.
    pub destination_endpoint: u8,
    /// [`crate::ids::aps_option`] bits.
    pub options: u16,
    /// Group id, for multicast.
    pub group_id: u16,
    /// APS counter.
    pub sequence: u8,
}

impl ApsFrame {
    /// Decode.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, Truncated> {
        Ok(ApsFrame {
            profile_id: r.u16()?,
            cluster_id: r.u16()?,
            source_endpoint: r.u8()?,
            destination_endpoint: r.u8()?,
            options: r.u16()?,
            group_id: r.u16()?,
            sequence: r.u8()?,
        })
    }

    /// Encode.
    pub fn write(&self, w: &mut Vec<u8>) {
        w.put_u16(self.profile_id);
        w.put_u16(self.cluster_id);
        w.put_u8(self.source_endpoint);
        w.put_u8(self.destination_endpoint);
        w.put_u16(self.options);
        w.put_u16(self.group_id);
        w.put_u8(self.sequence);
    }
}

/// How a message arrived: the sender and the last hop's link quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RxPacketInfo {
    /// The sender's network address.
    pub sender: NodeId,
    /// The sender's IEEE address, when the frame carried it (version 14+).
    pub sender_ieee: Option<Ieee>,
    /// Binding table index, 0xFF for none.
    pub binding_index: u8,
    /// Address table index, 0xFF for none.
    pub address_index: u8,
    /// Link quality of the last hop.
    pub last_hop_lqi: u8,
    /// Received signal strength of the last hop, dBm.
    pub last_hop_rssi: i8,
    /// Receive time of the last hop (version 14+).
    pub last_hop_timestamp: u32,
}

impl RxPacketInfo {
    /// Decode the version 14+ `EmberRxPacketInfo`.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, Truncated> {
        let sender = r.u16()?;
        let ieee = r.ieee()?;
        Ok(RxPacketInfo {
            sender,
            sender_ieee: (ieee != Ieee::ZERO && ieee != Ieee::UNSET).then_some(ieee),
            binding_index: r.u8()?,
            address_index: r.u8()?,
            last_hop_lqi: r.u8()?,
            last_hop_rssi: r.i8()?,
            last_hop_timestamp: r.u32()?,
        })
    }

    /// Encode the version 14+ `EmberRxPacketInfo`.
    pub fn write(&self, w: &mut Vec<u8>) {
        w.put_u16(self.sender);
        w.put_ieee(self.sender_ieee.unwrap_or(Ieee::ZERO));
        w.put_u8(self.binding_index);
        w.put_u8(self.address_index);
        w.put_u8(self.last_hop_lqi);
        w.put_u8(self.last_hop_rssi as u8);
        w.put_u32(self.last_hop_timestamp);
    }
}

/// `EmberNetworkParameters`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetworkParameters {
    /// Extended PAN id.
    pub extended_pan_id: ExtPanId,
    /// PAN id.
    pub pan_id: PanId,
    /// Transmit power, dBm.
    pub radio_tx_power: i8,
    /// Channel, 11 to 26.
    pub radio_channel: u8,
    /// `EmberJoinMethod`; 0 (MAC association) when forming.
    pub join_method: u8,
    /// Network manager's address.
    pub nwk_manager_id: NodeId,
    /// Network update id.
    pub nwk_update_id: u8,
    /// Channel mask.
    pub channels: u32,
}

impl NetworkParameters {
    /// Decode.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, Truncated> {
        Ok(NetworkParameters {
            extended_pan_id: r.ext_pan_id()?,
            pan_id: r.u16()?,
            radio_tx_power: r.i8()?,
            radio_channel: r.u8()?,
            join_method: r.u8()?,
            nwk_manager_id: r.u16()?,
            nwk_update_id: r.u8()?,
            channels: r.u32()?,
        })
    }

    /// Encode.
    pub fn write(&self, w: &mut Vec<u8>) {
        w.put_ext_pan_id(self.extended_pan_id);
        w.put_u16(self.pan_id);
        w.put_u8(self.radio_tx_power as u8);
        w.put_u8(self.radio_channel);
        w.put_u8(self.join_method);
        w.put_u16(self.nwk_manager_id);
        w.put_u8(self.nwk_update_id);
        w.put_u32(self.channels);
    }
}

/// `EmberInitialSecurityState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InitialSecurityState {
    /// [`crate::ids::security`] bits.
    pub bitmask: u16,
    /// Trust center link key (with hashed link keys, the root key).
    pub preconfigured_key: Key,
    /// Network key.
    pub network_key: Key,
    /// Network key sequence number.
    pub network_key_sequence: u8,
    /// Trust center IEEE address, or zero.
    pub preconfigured_trust_center: Ieee,
}

impl InitialSecurityState {
    /// Decode.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, Truncated> {
        Ok(InitialSecurityState {
            bitmask: r.u16()?,
            preconfigured_key: r.key()?,
            network_key: r.key()?,
            network_key_sequence: r.u8()?,
            preconfigured_trust_center: r.ieee()?,
        })
    }

    /// Encode.
    pub fn write(&self, w: &mut Vec<u8>) {
        w.put_u16(self.bitmask);
        w.put_key(&self.preconfigured_key);
        w.put_key(&self.network_key);
        w.put_u8(self.network_key_sequence);
        w.put_ieee(self.preconfigured_trust_center);
    }
}

/// `sl_zb_sec_man_context_t`: which key a security manager call means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SecManContext {
    /// [`crate::ids::key_type`].
    pub core_key_type: u8,
    /// Key table index.
    pub key_index: u8,
    /// Derived key type, 0 for none.
    pub derived_type: u16,
    /// The partner's IEEE address, for link keys.
    pub ieee: Ieee,
    /// Network index.
    pub multi_network_index: u8,
    /// Flags.
    pub flags: u8,
    /// PSA key algorithm permission.
    pub psa_key_alg_permission: u32,
}

impl SecManContext {
    /// A context for a key type, everything else zero.
    pub fn for_key(core_key_type: u8) -> Self {
        SecManContext {
            core_key_type,
            ..Default::default()
        }
    }

    /// Decode.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, Truncated> {
        Ok(SecManContext {
            core_key_type: r.u8()?,
            key_index: r.u8()?,
            derived_type: r.u16()?,
            ieee: r.ieee()?,
            multi_network_index: r.u8()?,
            flags: r.u8()?,
            psa_key_alg_permission: r.u32()?,
        })
    }

    /// Encode.
    pub fn write(&self, w: &mut Vec<u8>) {
        w.put_u8(self.core_key_type);
        w.put_u8(self.key_index);
        w.put_u16(self.derived_type);
        w.put_ieee(self.ieee);
        w.put_u8(self.multi_network_index);
        w.put_u8(self.flags);
        w.put_u32(self.psa_key_alg_permission);
    }
}

/// `sl_zb_sec_man_aps_key_metadata_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApsKeyMetadata {
    /// Key properties.
    pub bitmask: u16,
    /// Outgoing APS frame counter.
    pub outgoing_frame_counter: u32,
    /// Incoming APS frame counter.
    pub incoming_frame_counter: u32,
    /// Seconds left for a transient key.
    pub ttl_seconds: u16,
}

impl ApsKeyMetadata {
    /// Decode.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, Truncated> {
        Ok(ApsKeyMetadata {
            bitmask: r.u16()?,
            outgoing_frame_counter: r.u32()?,
            incoming_frame_counter: r.u32()?,
            ttl_seconds: r.u16()?,
        })
    }

    /// Encode.
    pub fn write(&self, w: &mut Vec<u8>) {
        w.put_u16(self.bitmask);
        w.put_u32(self.outgoing_frame_counter);
        w.put_u32(self.incoming_frame_counter);
        w.put_u16(self.ttl_seconds);
    }
}

/// `sl_zb_sec_man_network_key_info_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetworkKeyInfo {
    /// A network key is set.
    pub network_key_set: bool,
    /// An alternate network key is set.
    pub alternate_network_key_set: bool,
    /// Network key sequence number.
    pub network_key_sequence: u8,
    /// Alternate key sequence number.
    pub alternate_network_key_sequence: u8,
    /// Outgoing network frame counter.
    pub network_key_frame_counter: u32,
}

impl NetworkKeyInfo {
    /// Decode.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, Truncated> {
        Ok(NetworkKeyInfo {
            network_key_set: r.bool()?,
            alternate_network_key_set: r.bool()?,
            network_key_sequence: r.u8()?,
            alternate_network_key_sequence: r.u8()?,
            network_key_frame_counter: r.u32()?,
        })
    }

    /// Encode.
    pub fn write(&self, w: &mut Vec<u8>) {
        w.put_u8(u8::from(self.network_key_set));
        w.put_u8(u8::from(self.alternate_network_key_set));
        w.put_u8(self.network_key_sequence);
        w.put_u8(self.alternate_network_key_sequence);
        w.put_u32(self.network_key_frame_counter);
    }
}

/// `EmberVersion`, from the `VERSION_INFO` value: the EmberZNet release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StackVersion {
    /// Build number.
    pub build: u16,
    /// Major version.
    pub major: u8,
    /// Minor version.
    pub minor: u8,
    /// Patch version.
    pub patch: u8,
    /// Special version.
    pub special: u8,
    /// Release type: 0x00 pre-release, 0x33 alpha, 0x66 beta, 0xAA GA.
    pub kind: u8,
}

impl StackVersion {
    /// Decode the 7-byte `VERSION_INFO` value.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, Truncated> {
        Ok(StackVersion {
            build: r.u16()?,
            major: r.u8()?,
            minor: r.u8()?,
            patch: r.u8()?,
            special: r.u8()?,
            kind: r.u8()?,
        })
    }

    /// Encode as the 7-byte `VERSION_INFO` value.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Vec::with_capacity(7);
        w.put_u16(self.build);
        w.put_u8(self.major);
        w.put_u8(self.minor);
        w.put_u8(self.patch);
        w.put_u8(self.special);
        w.put_u8(self.kind);
        w
    }
}

impl fmt::Display for StackVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.{} build {}",
            self.major, self.minor, self.patch, self.special, self.build
        )?;
        match self.kind {
            0xAA => Ok(()),
            0x00 => f.write_str(" pre-release"),
            0x33 => f.write_str(" alpha"),
            0x66 => f.write_str(" beta"),
            other => write!(f, " (type {other:#04x})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_spaces() {
        let v13 = ProtocolVersion(13);
        let v16 = ProtocolVersion(16);
        assert!(Status::network_up(v13).is_network_up());
        assert_eq!(Status::network_up(v13).raw, 0x90);
        assert_eq!(Status::network_up(v16).raw, 0x15);
        assert!(!Status {
            raw: 0x90,
            legacy: false
        }
        .is_network_up());
        assert_eq!(Status::ok(v16).to_string(), "ok (0x0000)");
        assert_eq!(
            Status {
                raw: 0x42,
                legacy: true
            }
            .to_string(),
            "ember status 0x42"
        );
    }

    #[test]
    fn structures_round_trip() {
        let p = NetworkParameters {
            extended_pan_id: ExtPanId(0xDDDDDDDDDDDDDDDD),
            pan_id: 0x1A62,
            radio_tx_power: -3,
            radio_channel: 15,
            join_method: 0,
            nwk_manager_id: 0,
            nwk_update_id: 2,
            channels: 0x07FF_F800,
        };
        let mut w = Vec::new();
        p.write(&mut w);
        assert_eq!(w.len(), 20);
        assert_eq!(NetworkParameters::read(&mut Reader::new(&w)).unwrap(), p);
        let v = StackVersion {
            build: 380,
            major: 7,
            minor: 4,
            patch: 4,
            special: 0,
            kind: 0xAA,
        };
        assert_eq!(
            StackVersion::read(&mut Reader::new(&v.to_bytes())).unwrap(),
            v
        );
        assert_eq!(v.to_string(), "7.4.4.0 build 380");
    }
}
