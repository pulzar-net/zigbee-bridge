//! The device store: one JSON file, written atomically and only when a
//! device record changes, batched so a burst of changes is one write.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use zigbee_primitives::Ieee;

use crate::device::Device;

/// The store file's format version.
pub const STORE_VERSION: u32 = 1;

/// How long changes are gathered before one write.
pub const WRITE_DELAY: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    devices: Vec<Device>,
}

/// Store errors.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Reading or writing the file failed.
    #[error("device store {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The error.
        source: std::io::Error,
    },
    /// The file did not parse.
    #[error("device store {path}: {message}")]
    Parse {
        /// The file.
        path: PathBuf,
        /// What was wrong.
        message: String,
    },
}

/// Load devices from `path`; a missing file is an empty store.
pub fn load(path: &Path) -> Result<BTreeMap<Ieee, Device>, StoreError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(source) => {
            return Err(StoreError::Io {
                path: path.into(),
                source,
            })
        }
    };
    let file: StoreFile = serde_json::from_str(&text).map_err(|e| StoreError::Parse {
        path: path.into(),
        message: e.to_string(),
    })?;
    if file.version != STORE_VERSION {
        return Err(StoreError::Parse {
            path: path.into(),
            message: format!("unsupported store version {}", file.version),
        });
    }
    Ok(file.devices.into_iter().map(|d| (d.ieee, d)).collect())
}

/// Write `contents` to `path` atomically: a temporary file in the same
/// directory, synced, then renamed over the old one. On Unix the file is
/// readable by its owner only, since backups hold the network key.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    // A temporary name of its own, so concurrent writers never share one.
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.{n}.tmp", std::process::id()));
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        if let Ok(dir) = std::fs::File::open(dir) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// Serialize devices for the store file.
pub fn serialize(devices: &BTreeMap<Ieee, Device>) -> Vec<u8> {
    let file = StoreFile {
        version: STORE_VERSION,
        devices: devices.values().cloned().collect(),
    };
    serde_json::to_vec(&file).unwrap_or_default()
}

/// Wakes the writer when devices change.
#[derive(Clone, Debug, Default)]
pub struct Dirty {
    notify: Arc<Notify>,
}

impl Dirty {
    /// Mark the store changed.
    pub fn mark(&self) {
        self.notify.notify_one();
    }

    /// Wait for a change.
    pub async fn changed(&self) {
        self.notify.notified().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_without_volatile_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        assert!(load(&path).unwrap().is_empty());
        let mut d = Device::new(Ieee(7), 0x1234, 100);
        d.model = Some("SIM-TH1".into());
        d.last_lqi = Some(200);
        let mut map = BTreeMap::new();
        map.insert(d.ieee, d.clone());
        write_atomic(&path, &serialize(&map)).unwrap();
        let loaded = load(&path).unwrap();
        d.last_lqi = None;
        assert_eq!(loaded.get(&Ieee(7)), Some(&d));
        std::fs::write(&path, b"{").unwrap();
        assert!(load(&path).is_err());
    }
}
