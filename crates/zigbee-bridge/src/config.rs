//! The bridge's configuration file (TOML).

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The whole configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The coordinator's port.
    pub serial: SerialConfig,
    /// The MQTT broker.
    #[serde(default)]
    pub mqtt: MqttConfig,
    /// Home Assistant discovery.
    #[serde(default)]
    pub homeassistant: HomeAssistantConfig,
    /// The network to form on first start.
    #[serde(default)]
    pub network: NetworkConfig,
    /// Trust center join policy.
    #[serde(default)]
    pub join: JoinConfig,
    /// Where state lives.
    #[serde(default)]
    pub data: DataConfig,
}

/// The coordinator's port.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SerialConfig {
    /// A serial device (`/dev/ttyUSB0`) or `tcp://host:port`.
    pub port: String,
    /// Baud rate.
    #[serde(default = "default_baud")]
    pub baud: u32,
    /// `none`, `hardware` (RTS/CTS) or `software`.
    #[serde(default)]
    pub flow_control: FlowControl,
    /// The coordinator family: `auto` (probe for it), `ezsp` or `znp`.
    #[serde(default)]
    pub adapter: AdapterChoice,
}

/// Which coordinator family to speak.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AdapterChoice {
    /// Probe the port for each family.
    #[default]
    Auto,
    /// Silicon Labs EmberZNet.
    Ezsp,
    /// Texas Instruments Z-Stack.
    Znp,
}

/// Serial flow control.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FlowControl {
    /// None.
    #[default]
    None,
    /// RTS/CTS.
    Hardware,
    /// XON/XOFF.
    Software,
}

fn default_baud() -> u32 {
    115_200
}

/// The MQTT broker.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MqttConfig {
    /// Broker host.
    pub host: String,
    /// Broker port.
    pub port: u16,
    /// User name, if the broker wants one.
    pub username: Option<String>,
    /// A file holding the password (so it is not in the configuration).
    pub password_file: Option<PathBuf>,
    /// Client id.
    pub client_id: String,
    /// Prefix of every topic the bridge uses.
    pub base_topic: String,
    /// Keep-alive, seconds.
    pub keepalive: u64,
    /// Publish device state retained, so a subscriber sees it at once.
    pub retain_state: bool,
}

impl Default for MqttConfig {
    fn default() -> Self {
        MqttConfig {
            host: "localhost".into(),
            port: 1883,
            username: None,
            password_file: None,
            client_id: "zigbee-bridge".into(),
            base_topic: "zigbee".into(),
            keepalive: 30,
            retain_state: true,
        }
    }
}

/// Home Assistant discovery.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HomeAssistantConfig {
    /// Publish discovery messages.
    pub enabled: bool,
    /// Home Assistant's discovery prefix.
    pub discovery_prefix: String,
    /// Home Assistant's birth topic; discovery is re-sent when it says "online".
    pub status_topic: String,
}

impl Default for HomeAssistantConfig {
    fn default() -> Self {
        HomeAssistantConfig {
            enabled: true,
            discovery_prefix: "homeassistant".into(),
            status_topic: "homeassistant/status".into(),
        }
    }
}

/// The network formed on first start. After that the network file in the
/// data directory is the source of truth.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct NetworkConfig {
    /// Channel, 11 to 26.
    pub channel: u8,
    /// Transmit power in dBm, if not the firmware's default.
    pub tx_power: Option<i8>,
    /// Leave a different network found on the coordinator and form this
    /// one. Off by default: a coordinator with another network is never
    /// wiped without being told to.
    pub replace_other: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            channel: 15,
            tx_power: None,
            replace_other: false,
        }
    }
}

/// Trust center join policy.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct JoinConfig {
    /// Only devices with an install code may join.
    pub install_code_only: bool,
    /// Accept pre-Zigbee 3.0 devices (they keep the well-known link key).
    pub allow_legacy: bool,
}

impl Default for JoinConfig {
    fn default() -> Self {
        JoinConfig {
            install_code_only: false,
            allow_legacy: true,
        }
    }
}

/// Where state lives.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DataConfig {
    /// The data directory: network keys, device store, backup.
    pub dir: PathBuf,
}

impl Default for DataConfig {
    fn default() -> Self {
        DataConfig {
            dir: PathBuf::from("/var/lib/zigbee-bridge"),
        }
    }
}

/// A configuration error.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("reading {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The file did not parse.
    #[error("{path}: {message}")]
    Parse {
        /// The file.
        path: PathBuf,
        /// Why.
        message: String,
    },
}

impl Config {
    /// Parse a configuration.
    pub fn from_toml(text: &str) -> Result<Config, String> {
        let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        if !(11..=26).contains(&config.network.channel) {
            return Err(format!(
                "network.channel {} is not 11 to 26",
                config.network.channel
            ));
        }
        if config.mqtt.base_topic.is_empty() || config.mqtt.base_topic.contains(['+', '#']) {
            return Err("mqtt.base_topic must be a plain topic prefix".into());
        }
        Ok(config)
    }

    /// Read and parse a configuration file.
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.into(),
            source,
        })?;
        Config::from_toml(&text).map_err(|message| ConfigError::Parse {
            path: path.into(),
            message,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_and_full_configs_parse() {
        let c = Config::from_toml("[serial]\nport = \"/dev/ttyUSB0\"\n").unwrap();
        assert_eq!(c.serial.baud, 115_200);
        assert_eq!(c.mqtt.base_topic, "zigbee");
        assert!(c.homeassistant.enabled);
        let c = Config::from_toml(
            "[serial]\nport = \"tcp://10.0.0.5:6638\"\nflow_control = \"hardware\"\n[mqtt]\nhost = \"broker\"\nusername = \"z\"\n[network]\nchannel = 25\n[join]\ninstall_code_only = true\n[data]\ndir = \"/tmp/z\"\n",
        )
        .unwrap();
        assert_eq!(c.serial.flow_control, FlowControl::Hardware);
        assert_eq!(c.network.channel, 25);
        assert!(Config::from_toml("[serial]\nport = \"x\"\n[network]\nchannel = 5\n").is_err());
        assert!(Config::from_toml("[serial]\nport = \"x\"\nbogus = 1\n").is_err());
    }
}
