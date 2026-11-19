//! Network backups in the open coordinator backup format (version 1), the
//! JSON format several Zigbee hosts share, so a backup names everything a
//! replacement coordinator needs: the network key and its frame counter,
//! the PAN ids and channel, the coordinator's IEEE address and the trust
//! center's link keys.

use serde::{Deserialize, Serialize};
use zigbee_types::{ExtPanId, Ieee, Key, PanId};

/// The format name in `metadata.format`.
pub const FORMAT: &str = "zigpy/open-coordinator-backup";

/// How far the network frame counter is advanced on restore, so that the
/// restored coordinator never reuses a counter the old one sent after the
/// backup was taken.
pub const RESTORE_FRAME_COUNTER_MARGIN: u32 = 100_000;

/// A network backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backup {
    /// Format, version and producer.
    pub metadata: Metadata,
    /// Stack-specific data needed to restore onto the same NCP family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_specific: Option<StackSpecific>,
    /// The coordinator's IEEE address.
    pub coordinator_ieee: Ieee,
    /// PAN id, as four hex digits.
    #[serde(with = "pan_hex")]
    pub pan_id: PanId,
    /// Extended PAN id.
    pub extended_pan_id: ExtPanId,
    /// Network update id.
    pub nwk_update_id: u8,
    /// Security level (5).
    pub security_level: u8,
    /// Channel.
    pub channel: u8,
    /// Channels the network may use.
    pub channel_mask: Vec<u8>,
    /// The network key.
    pub network_key: NetworkKey,
    /// Devices and their link keys.
    pub devices: Vec<BackupDevice>,
}

/// Backup metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    /// [`FORMAT`].
    pub format: String,
    /// 1.
    pub version: u32,
    /// The producer, `name@version`.
    pub source: String,
    /// Producer-specific details.
    #[serde(default)]
    pub internal: Internal,
}

/// Producer-specific metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Internal {
    /// When the backup was taken, seconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_unix: Option<u64>,
    /// The NCP's EZSP protocol version.
    #[serde(
        default,
        rename = "ezspVersion",
        skip_serializing_if = "Option::is_none"
    )]
    pub ezsp_version: Option<u8>,
    /// The NCP's firmware version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware: Option<String>,
}

/// Stack-specific data.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StackSpecific {
    /// For EmberZNet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ezsp: Option<EzspSpecific>,
}

/// EmberZNet-specific data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EzspSpecific {
    /// The trust center's root key, from which it derives each device's
    /// unique link key (hashed link keys).
    pub hashed_tclk: Key,
}

/// The network key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkKey {
    /// The key.
    pub key: Key,
    /// Its sequence number.
    pub sequence_number: u8,
    /// The coordinator's outgoing frame counter.
    pub frame_counter: u32,
}

/// One device in a backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupDevice {
    /// Network address, four hex digits, if known.
    #[serde(default, with = "opt_pan_hex")]
    pub nwk_address: Option<u16>,
    /// IEEE address.
    pub ieee_address: Ieee,
    /// A child of the coordinator.
    #[serde(default)]
    pub is_child: bool,
    /// Its trust center link key, if the trust center keeps one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_key: Option<LinkKey>,
}

/// A device's link key and frame counters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkKey {
    /// The key.
    pub key: Key,
    /// Outgoing APS frame counter.
    pub tx_counter: u32,
    /// Incoming APS frame counter.
    pub rx_counter: u32,
}

/// A backup failed validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackupError {
    /// Not the open coordinator backup format.
    #[error("not an open coordinator backup (format {0:?})")]
    Format(String),
    /// An unsupported format version.
    #[error("unsupported backup format version {0}")]
    Version(u32),
    /// The channel is not 11 to 26.
    #[error("channel {0} is not a 2.4 GHz Zigbee channel")]
    Channel(u8),
    /// The JSON did not parse.
    #[error("backup JSON: {0}")]
    Json(String),
}

impl Backup {
    /// Parse and validate a backup.
    pub fn from_json(text: &str) -> Result<Backup, BackupError> {
        let backup: Backup =
            serde_json::from_str(text).map_err(|e| BackupError::Json(e.to_string()))?;
        backup.validate()?;
        Ok(backup)
    }

    /// Check the fields a restore relies on.
    pub fn validate(&self) -> Result<(), BackupError> {
        if self.metadata.format != FORMAT {
            return Err(BackupError::Format(self.metadata.format.clone()));
        }
        if self.metadata.version != 1 {
            return Err(BackupError::Version(self.metadata.version));
        }
        if !(11..=26).contains(&self.channel) {
            return Err(BackupError::Channel(self.channel));
        }
        Ok(())
    }

    /// The JSON text.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// The frame counter a restored coordinator should start from.
    pub fn restore_frame_counter(&self) -> u32 {
        self.network_key
            .frame_counter
            .saturating_add(RESTORE_FRAME_COUNTER_MARGIN)
    }
}

mod pan_hex {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u16, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{v:04x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
        let text = String::deserialize(d)?;
        u16::from_str_radix(text.trim_start_matches("0x"), 16).map_err(D::Error::custom)
    }
}

mod opt_pan_hex {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<u16>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(v) => s.serialize_str(&format!("{v:04x}")),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u16>, D::Error> {
        match Option::<String>::deserialize(d)? {
            None => Ok(None),
            Some(text) => u16::from_str_radix(text.trim_start_matches("0x"), 16)
                .map(Some)
                .map_err(D::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_validates() {
        let backup = Backup {
            metadata: Metadata {
                format: FORMAT.into(),
                version: 1,
                source: "zigbee-bridge@0.1.0".into(),
                internal: Internal {
                    created_unix: Some(1),
                    ezsp_version: Some(13),
                    firmware: Some("7.4.4.0".into()),
                },
            },
            stack_specific: Some(StackSpecific {
                ezsp: Some(EzspSpecific {
                    hashed_tclk: Key([1; 16]),
                }),
            }),
            coordinator_ieee: Ieee(0x00124B0001020304),
            pan_id: 0x1A62,
            extended_pan_id: ExtPanId(0xDDDDDDDDDDDDDDDD),
            nwk_update_id: 0,
            security_level: 5,
            channel: 15,
            channel_mask: vec![15],
            network_key: NetworkKey {
                key: Key([2; 16]),
                sequence_number: 0,
                frame_counter: 4242,
            },
            devices: vec![BackupDevice {
                nwk_address: Some(0x1234),
                ieee_address: Ieee(5),
                is_child: true,
                link_key: Some(LinkKey {
                    key: Key([3; 16]),
                    tx_counter: 1,
                    rx_counter: 2,
                }),
            }],
        };
        let json = backup.to_json();
        assert!(json.contains("\"pan_id\": \"1a62\""));
        assert!(json.contains("\"coordinator_ieee\": \"00124b0001020304\""));
        assert_eq!(Backup::from_json(&json).unwrap(), backup);
        assert_eq!(
            backup.restore_frame_counter(),
            4242 + RESTORE_FRAME_COUNTER_MARGIN
        );
        let bad = json.replace("\"channel\": 15", "\"channel\": 30");
        assert_eq!(Backup::from_json(&bad), Err(BackupError::Channel(30)));
    }
}
