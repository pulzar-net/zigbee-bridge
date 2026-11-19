//! The network file: the keys and PAN ids the bridge runs, in the data
//! directory, readable by its owner only. The bridge writes it with random
//! values on first start; a platform that manages keys writes it before.

use std::path::Path;

use serde::{Deserialize, Serialize};
use zigbee_host::{Backup, NetworkSettings};
use zigbee_types::{ExtPanId, Key};

/// The file's name in the data directory.
pub const FILE: &str = "network.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkFile {
    channel: u8,
    pan_id: String,
    extended_pan_id: ExtPanId,
    network_key: Key,
}

/// Read the network file, or create it with `channel` and random ids and
/// key when there is none.
pub fn load_or_create(
    dir: &Path,
    channel: u8,
    tx_power: Option<i8>,
) -> Result<(NetworkSettings, bool), String> {
    let path = dir.join(FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let f: NetworkFile =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let pan_id = u16::from_str_radix(f.pan_id.trim_start_matches("0x"), 16)
                .map_err(|e| format!("{}: pan_id: {e}", path.display()))?;
            Ok((
                NetworkSettings {
                    channel: f.channel,
                    pan_id,
                    extended_pan_id: f.extended_pan_id,
                    network_key: f.network_key,
                    tx_power,
                },
                false,
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut settings = zigbee_host::random_network(channel).map_err(|e| e.to_string())?;
            settings.tx_power = tx_power;
            save(dir, &settings)?;
            Ok((settings, true))
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write the network file (mode 0600 on Unix).
pub fn save(dir: &Path, settings: &NetworkSettings) -> Result<(), String> {
    let f = NetworkFile {
        channel: settings.channel,
        pan_id: format!("{:04x}", settings.pan_id),
        extended_pan_id: settings.extended_pan_id,
        network_key: settings.network_key,
    };
    let text = serde_json::to_string_pretty(&f).map_err(|e| e.to_string())?;
    let path = dir.join(FILE);
    zigbee_host::store::write_atomic(&path, text.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    restrict(&path);
    Ok(())
}

/// The network a backup describes.
pub fn from_backup(backup: &Backup) -> NetworkSettings {
    NetworkSettings {
        channel: backup.channel,
        pan_id: backup.pan_id,
        extended_pan_id: backup.extended_pan_id,
        network_key: backup.network_key.key,
        tx_power: None,
    }
}

/// Make a file readable by its owner only.
pub fn restrict(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_once_then_reads_the_same_network() {
        let dir = tempfile::tempdir().unwrap();
        let (first, created) = load_or_create(dir.path(), 20, None).unwrap();
        assert!(created);
        let (again, created) = load_or_create(dir.path(), 11, Some(3)).unwrap();
        assert!(!created);
        assert_eq!(again.network_key, first.network_key);
        assert_eq!(again.channel, 20);
        assert_eq!(again.tx_power, Some(3));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
    }
}
