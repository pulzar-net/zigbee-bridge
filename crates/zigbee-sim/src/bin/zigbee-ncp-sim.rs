//! Serve the NCP simulator over TCP.
//!
//! ```text
//! zigbee-ncp-sim [--listen 127.0.0.1:9999] [--ncp ezsp|znp|deconz] [--protocol 13] [--devices th,plug,door] [--report-every 30]
//! ```
//!
//! Point a host at `tcp://127.0.0.1:9999`. Devices join when the host opens
//! the network; with `--report-every`, sensors drift and report.

use std::time::Duration;

use zigbee_primitives::Ieee;
use zigbee_sim::{SimConfig, SimDevice, SimNcp, Simulator};
use zigbee_zcl::{attr, cluster_id, Value};

fn usage() -> ! {
    eprintln!("usage: zigbee-ncp-sim [--listen ADDR] [--ncp ezsp|znp|deconz] [--protocol 13..19] [--devices th,plug,door] [--report-every SECONDS]");
    std::process::exit(2)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut listen = "127.0.0.1:9999".to_string();
    let mut config = SimConfig::default();
    let mut kinds = vec!["th".to_string(), "plug".to_string(), "door".to_string()];
    let mut report_every: Option<u64> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("zigbee-ncp-sim {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--listen" => listen = args.next().unwrap_or_else(|| usage()),
            "--ncp" => {
                config.ncp = match args.next().as_deref() {
                    Some("ezsp") => SimNcp::Ezsp { protocol: 13 },
                    Some("znp") => SimNcp::Znp { product: 1 },
                    Some("deconz") => SimNcp::Deconz,
                    _ => usage(),
                }
            }
            "--protocol" => {
                let protocol = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage());
                config.ncp = SimNcp::Ezsp { protocol };
            }
            "--devices" => {
                kinds = args
                    .next()
                    .unwrap_or_else(|| usage())
                    .split(',')
                    .map(str::to_string)
                    .collect()
            }
            "--report-every" => report_every = args.next().and_then(|v| v.parse().ok()),
            _ => usage(),
        }
    }
    for (i, kind) in kinds.iter().enumerate() {
        let ieee = Ieee(0x00_12_4B_00_00_00_00_00 | (i as u64 + 1));
        config.devices.push(match kind.as_str() {
            "th" => SimDevice::temperature_humidity_sensor(ieee),
            "plug" => SimDevice::plug(ieee),
            "door" => SimDevice::contact_sensor(ieee),
            _ => usage(),
        });
    }
    let sensors: Vec<Ieee> = config
        .devices
        .iter()
        .filter(|d| {
            d.endpoints
                .iter()
                .any(|e| e.servers.contains_key(&cluster_id::TEMPERATURE_MEASUREMENT))
        })
        .map(|d| d.ieee)
        .collect();
    let config_ncp = config.ncp;
    let sim = Simulator::new(config);
    if let Some(seconds) = report_every {
        let sim = sim.clone();
        tokio::spawn(async move {
            let mut step: i16 = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                step = (step + 1) % 20;
                for ieee in &sensors {
                    let temperature = 2000 + (step - 10) * 15;
                    sim.set_attribute(
                        *ieee,
                        1,
                        cluster_id::TEMPERATURE_MEASUREMENT,
                        attr::temperature_measurement::MEASURED_VALUE,
                        Value::i16(temperature),
                    );
                    sim.set_attribute(
                        *ieee,
                        1,
                        cluster_id::RELATIVE_HUMIDITY_MEASUREMENT,
                        attr::relative_humidity_measurement::MEASURED_VALUE,
                        Value::u16(4500 + (step as u16) * 20),
                    );
                }
            }
        });
    }
    let listener = match tokio::net::TcpListener::bind(&listen).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("zigbee-ncp-sim: listening on {listen}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!(%listen, ncp = ?config_ncp, "NCP simulator listening");
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    tracing::info!(%peer, "host connected");
                    let _ = stream.set_nodelay(true);
                    if let Err(e) = sim.serve(stream).await {
                        tracing::info!(error = %e, "host disconnected");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "accept"),
            },
            _ = tokio::signal::ctrl_c() => return,
        }
    }
}
