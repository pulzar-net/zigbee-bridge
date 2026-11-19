//! The memory budget: under 16 MB resident with 200 devices.
//!
//! The host and the simulator run in one process here, so the figure is an
//! upper bound for the host alone. Run in release mode on Linux:
//! `cargo test --release -p zigbee-host --test budget -- --ignored --nocapture`.

use std::time::Duration;

use zigbee_ezsp::ClientConfig;
use zigbee_host::{random_network, EzspAdapter, Host, HostConfig, HostEvent, StartMode};
use zigbee_sim::{SimConfig, SimDevice, Simulator};
use zigbee_types::Ieee;

const DEVICES: u64 = 200;
const BUDGET_KB: u64 = 16 * 1024;

fn status_kb(field: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|l| l.starts_with(field))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[test]
#[ignore = "budget measurement; run in release mode on Linux"]
fn two_hundred_devices_fit_the_memory_budget() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let devices = (0..DEVICES)
            .map(|i| {
                let ieee = Ieee(0x00124B0000100000 + i);
                if i % 2 == 0 {
                    SimDevice::temperature_humidity_sensor(ieee)
                } else {
                    SimDevice::plug(ieee)
                }
            })
            .collect();
        let sim = Simulator::new(SimConfig { devices, reply_delay: Duration::from_millis(5), ..SimConfig::default() });
        let (host_io, ncp_io) = tokio::io::duplex(16 * 1024);
        let s = sim.clone();
        tokio::spawn(async move {
            let _ = s.serve(ncp_io).await;
        });
        let dir = tempfile::tempdir().unwrap();
        let config = HostConfig { store_path: Some(dir.path().join("devices.json")), ..HostConfig::default() };
        let (adapter, events) = EzspAdapter::connect(host_io, ClientConfig::default()).await.unwrap();
        let mode = StartMode::Normal { settings: random_network(15).unwrap(), replace_other: false };
        let (host, mut rx) = Host::start(adapter, events, mode, config).await.unwrap();
        host.permit_join(254, None).await.unwrap();
        let mut done = 0;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
        while done < DEVICES {
            match tokio::time::timeout_at(deadline, rx.recv()).await.expect("all interviews in time").expect("host running") {
                HostEvent::InterviewDone { .. } => done += 1,
                HostEvent::InterviewFailed { ieee, stage, error } => panic!("{ieee} failed at {stage:?}: {error}"),
                _ => {}
            }
        }
        assert_eq!(host.devices().len() as u64, DEVICES);
        host.flush().await.unwrap();
        let store = std::fs::metadata(dir.path().join("devices.json")).unwrap().len();
        match (status_kb("VmRSS:"), status_kb("VmHWM:")) {
            (Some(rss), Some(peak)) => {
                println!("{DEVICES} devices: resident {rss} kB, peak {peak} kB, store {store} bytes");
                assert!(rss < BUDGET_KB, "resident {rss} kB is over the {BUDGET_KB} kB budget");
            }
            _ => println!("{DEVICES} devices interviewed; no /proc here to measure memory (store {store} bytes)"),
        }
    });
}
