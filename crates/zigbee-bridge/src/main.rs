//! `zigbee-bridge`: run the bridge, probe a port, or stage a restore.

use std::path::PathBuf;
use std::process::ExitCode;

use zigbee_bridge::{Config, RunError};

const USAGE: &str = "\
usage:
  zigbee-bridge [--config FILE]                 run the bridge
  zigbee-bridge probe PORT [--baud N]           say what coordinator is on PORT
  zigbee-bridge restore BACKUP [--config FILE]  restore BACKUP onto the coordinator at the next start
  zigbee-bridge --version

FILE defaults to /etc/zigbee-bridge/config.toml. PORT is a serial device or tcp://host:port.";

enum Action {
    Run,
    Probe { port: String, baud: u32 },
    Restore { backup: PathBuf },
    Version,
    Help,
}

fn parse() -> Result<(Action, PathBuf), lexopt::Error> {
    use lexopt::prelude::*;
    let mut parser = lexopt::Parser::from_env();
    let mut config = PathBuf::from("/etc/zigbee-bridge/config.toml");
    let mut action = Action::Run;
    let mut baud = 115_200;
    let mut sub: Option<String> = None;
    let mut positional: Option<String> = None;
    while let Some(arg) = parser.next()? {
        match arg {
            Short('c') | Long("config") => config = parser.value()?.into(),
            Long("baud") => baud = parser.value()?.parse()?,
            Short('V') | Long("version") => action = Action::Version,
            Short('h') | Long("help") => action = Action::Help,
            Value(v) if sub.is_none() => sub = Some(v.string()?),
            Value(v) if positional.is_none() => positional = Some(v.string()?),
            _ => return Err(arg.unexpected()),
        }
    }
    if matches!(action, Action::Version | Action::Help) {
        return Ok((action, config));
    }
    action = match (sub.as_deref(), positional) {
        (None, None) => Action::Run,
        (Some("probe"), Some(port)) => Action::Probe { port, baud },
        (Some("restore"), Some(backup)) => Action::Restore {
            backup: backup.into(),
        },
        _ => Action::Help,
    };
    Ok((action, config))
}

fn load(path: &std::path::Path) -> Result<Config, ExitCode> {
    Config::load(path).map_err(|e| {
        eprintln!("zigbee-bridge: {e}");
        ExitCode::from(2)
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let (action, config_path) = match parse() {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("zigbee-bridge: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match action {
        Action::Version => {
            println!("zigbee-bridge {}", zigbee_bridge::bridge::VERSION);
            ExitCode::SUCCESS
        }
        Action::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Action::Probe { port, baud } => match zigbee_host::probe::probe_any(&port, baud).await {
            Ok(Some(id)) => {
                println!(
                    "{}",
                    serde_json::json!({
                        "kind": id.kind.name(), "firmware": id.firmware, "protocol": id.protocol, "supported": id.supported,
                        "ieee_address": id.ieee.map(|i| i.to_prefixed_hex()), "manufacturer": id.manufacturer, "board": id.board,
                        "product": id.product,
                    })
                );
                ExitCode::SUCCESS
            }
            Ok(None) => {
                eprintln!(
                    "zigbee-bridge: no EZSP or ZNP coordinator answered on {port} at {baud} baud"
                );
                ExitCode::from(1)
            }
            Err(e) => {
                eprintln!("zigbee-bridge: {port}: {e}");
                ExitCode::from(1)
            }
        },
        Action::Restore { backup } => {
            let config = match load(&config_path) {
                Ok(c) => c,
                Err(code) => return code,
            };
            let text = match std::fs::read_to_string(&backup) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("zigbee-bridge: {}: {e}", backup.display());
                    return ExitCode::from(1);
                }
            };
            match zigbee_bridge::stage_restore(&config, &text) {
                Ok(b) => {
                    println!(
                        "restore of PAN {:04x} (coordinator {}) staged; it runs at the next start",
                        b.pan_id, b.coordinator_ieee
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("zigbee-bridge: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Action::Run => {
            let config = match load(&config_path) {
                Ok(c) => c,
                Err(code) => return code,
            };
            let stop = async {
                let _ = tokio::signal::ctrl_c().await;
            };
            #[cfg(unix)]
            let stop = async move {
                let mut term =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        Ok(t) => t,
                        Err(_) => return stop.await,
                    };
                tokio::select! {
                    _ = stop => {}
                    _ = term.recv() => {}
                }
            };
            match zigbee_bridge::run(config, stop).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(RunError::RestartRequired) => {
                    eprintln!("zigbee-bridge: {}", RunError::RestartRequired);
                    // EX_TEMPFAIL: a supervisor restarts the bridge.
                    ExitCode::from(75)
                }
                Err(e) => {
                    eprintln!("zigbee-bridge: {e}");
                    ExitCode::from(1)
                }
            }
        }
    }
}
