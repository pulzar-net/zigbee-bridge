#![doc = include_str!("../README.md")]

pub mod bridge;
pub mod config;
pub mod exposes;
pub mod mqtt;
pub mod network;

use std::path::Path;

use zigbee_host::{
    AnyAdapter, Backup, Host, HostConfig, HostError, JoinPolicy, NcpKind, StartMode,
};

pub use config::Config;

/// The device store's file name in the data directory.
pub const STORE_FILE: &str = "devices.json";
/// The latest backup's file name in the data directory.
pub const BACKUP_FILE: &str = "backup.json";
/// A backup staged by `zigbee-bridge restore`, restored on the next start.
pub const RESTORE_FILE: &str = "restore-pending.json";

/// Why the bridge stopped.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Bad configuration or data directory.
    #[error("{0}")]
    Setup(String),
    /// The coordinator could not be opened or started.
    #[error("coordinator: {0}")]
    Coordinator(#[from] HostError),
    /// A restore wrote the coordinator's IEEE address; start again to
    /// finish it.
    #[error(
        "the coordinator's IEEE address was written for the restore; start again to finish it"
    )]
    RestartRequired,
    /// The bridge stopped running.
    #[error("{0}")]
    Stopped(String),
}

/// Stage a backup to be restored onto the coordinator on the next start.
pub fn stage_restore(config: &Config, backup_json: &str) -> Result<Backup, RunError> {
    let backup = Backup::from_json(backup_json).map_err(|e| RunError::Setup(e.to_string()))?;
    let dir = &config.data.dir;
    create_dir(dir)?;
    zigbee_host::store::write_atomic(&dir.join(RESTORE_FILE), backup_json.as_bytes())
        .map_err(|e| RunError::Setup(e.to_string()))?;
    Ok(backup)
}

fn create_dir(dir: &Path) -> Result<(), RunError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| RunError::Setup(format!("data directory {}: {e}", dir.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Probe `port` for each coordinator family.
async fn detect(port: &str, baud: u32) -> Result<NcpKind, RunError> {
    match zigbee_host::probe::probe_any(port, baud).await {
        Ok(Some(id)) if id.supported => {
            tracing::info!(kind = id.kind.name(), firmware = %id.firmware, "coordinator detected");
            Ok(id.kind)
        }
        Ok(Some(id)) => Err(RunError::Setup(format!(
            "{port}: {} coordinator firmware {} is too old; update it (EmberZNet 7.4 or newer, Z-Stack 3, deCONZ 0x26580700 or newer)",
            id.kind.name(),
            id.firmware
        ))),
        Ok(None) => Err(RunError::Setup(format!(
            "{port}: no EZSP, ZNP or deCONZ coordinator answered at {baud} baud (set serial.adapter to skip detection)"
        ))),
        Err(e) => Err(RunError::Setup(format!("{port}: {e}"))),
    }
}

/// Run the bridge until `stop` resolves.
pub async fn run(
    config: Config,
    stop: impl std::future::Future<Output = ()>,
) -> Result<(), RunError> {
    let dir = config.data.dir.clone();
    create_dir(&dir)?;

    let restore_path = dir.join(RESTORE_FILE);
    let mode = match std::fs::read_to_string(&restore_path) {
        Ok(text) => {
            let backup = Backup::from_json(&text)
                .map_err(|e| RunError::Setup(format!("{}: {e}", restore_path.display())))?;
            tracing::warn!(pan = format!("{:04x}", backup.pan_id), ieee = %backup.coordinator_ieee, "restoring a backup onto the coordinator");
            network::save(&dir, &network::from_backup(&backup)).map_err(RunError::Setup)?;
            StartMode::Restore(Box::new(backup))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let (settings, created) =
                network::load_or_create(&dir, config.network.channel, config.network.tx_power)
                    .map_err(RunError::Setup)?;
            if created {
                tracing::info!(
                    channel = settings.channel,
                    pan = format!("{:04x}", settings.pan_id),
                    "new network settings written"
                );
            }
            StartMode::Normal {
                settings,
                replace_other: config.network.replace_other,
            }
        }
        Err(e) => return Err(RunError::Setup(format!("{}: {e}", restore_path.display()))),
    };
    let restoring = matches!(mode, StartMode::Restore(_));

    let flow = match config.serial.flow_control {
        config::FlowControl::None => zigbee_host::port::FlowControl::None,
        config::FlowControl::Hardware => zigbee_host::port::FlowControl::Hardware,
        config::FlowControl::Software => zigbee_host::port::FlowControl::Software,
    };
    let kind = match config.serial.adapter {
        config::AdapterChoice::Ezsp => NcpKind::Ezsp,
        config::AdapterChoice::Znp => NcpKind::Znp,
        config::AdapterChoice::Deconz => NcpKind::Deconz,
        config::AdapterChoice::Auto => detect(&config.serial.port, config.serial.baud).await?,
    };
    let port = zigbee_host::port::open(&config.serial.port, config.serial.baud, flow)
        .await
        .map_err(|e| RunError::Setup(format!("opening {}: {e}", config.serial.port)))?;
    let (adapter, adapter_events) = AnyAdapter::connect(port, kind).await?;
    let host_config = HostConfig {
        store_path: Some(dir.join(STORE_FILE)),
        backup_path: Some(dir.join(BACKUP_FILE)),
        join: JoinPolicy {
            install_code_only: config.join.install_code_only,
            allow_legacy: config.join.allow_legacy,
        },
        ota_dir: Some(config.ota.dir.clone().unwrap_or_else(|| dir.join("ota"))),
        ..HostConfig::default()
    };
    let (host, host_events) = match Host::start(adapter, adapter_events, mode, host_config).await {
        Ok(started) => started,
        Err(HostError::RestartRequired) => return Err(RunError::RestartRequired),
        Err(e) => return Err(e.into()),
    };
    if restoring {
        let _ = std::fs::remove_file(&restore_path);
        tracing::info!("restore finished");
    }
    let (mqtt, mqtt_in) = mqtt::connect(&config.mqtt).map_err(RunError::Setup)?;
    let bridge = bridge::Bridge::new(host, mqtt, config);
    bridge::run_loop(bridge, host_events, mqtt_in, stop)
        .await
        .map_err(RunError::Stopped)
}
