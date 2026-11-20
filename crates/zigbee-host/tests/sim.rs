//! The host against the NCP simulator, for every coordinator family and
//! layout: EZSP protocol 13 (EmberZNet 7.4) and 16 (8.x), and Z-Stack
//! 3.x.0 and 3.0.x over ZNP.

use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use zigbee_host::probe::NcpKind;
use zigbee_host::{
    random_network, AnyAdapter, Backup, Host, HostConfig, HostError, HostEvent, InterviewStage,
    JoinPolicy, NetworkSettings, StartMode, StartOutcome,
};
use zigbee_sim::{SimConfig, SimDevice, SimNcp, Simulator};
use zigbee_types::Ieee;
use zigbee_zcl::{attr, cluster_id, Value};

const TH: Ieee = Ieee(0x00124B0000000001);
const DOOR: Ieee = Ieee(0x00124B0000000002);
const PLUG: Ieee = Ieee(0x00124B0000000003);
const NCPS: [SimNcp; 5] = [
    SimNcp::Ezsp { protocol: 13 },
    SimNcp::Ezsp { protocol: 16 },
    SimNcp::Znp { product: 1 },
    SimNcp::Znp { product: 2 },
    SimNcp::Deconz,
];

fn kind(ncp: SimNcp) -> NcpKind {
    match ncp {
        SimNcp::Ezsp { .. } => NcpKind::Ezsp,
        SimNcp::Znp { .. } => NcpKind::Znp,
        SimNcp::Deconz => NcpKind::Deconz,
    }
}

fn sim(ncp: SimNcp, devices: Vec<SimDevice>) -> Simulator {
    Simulator::new(SimConfig {
        ncp,
        devices,
        ..SimConfig::default()
    })
}

fn config(dir: &std::path::Path) -> HostConfig {
    HostConfig {
        store_path: Some(dir.join("devices.json")),
        backup_path: Some(dir.join("backup.json")),
        request_timeout: Duration::from_secs(2),
        ..HostConfig::default()
    }
}

async fn start(
    sim: &Simulator,
    ncp: SimNcp,
    mode: StartMode,
    config: HostConfig,
) -> Result<(Host<AnyAdapter>, UnboundedReceiver<HostEvent>), HostError> {
    let (host_io, ncp_io) = tokio::io::duplex(4096);
    let sim = sim.clone();
    tokio::spawn(async move {
        let _ = sim.serve(ncp_io).await;
    });
    let (adapter, events) = AnyAdapter::connect(host_io, kind(ncp)).await?;
    Host::start(adapter, events, mode, config).await
}

async fn wait_for<T>(
    rx: &mut UnboundedReceiver<HostEvent>,
    mut f: impl FnMut(&HostEvent) -> Option<T>,
) -> T {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("timed out waiting for a host event")
            .expect("host stopped");
        if let HostEvent::InterviewFailed { ieee, stage, error } = &event {
            panic!("interview of {ieee} failed at {stage:?}: {error}");
        }
        if let Some(v) = f(&event) {
            return v;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forms_joins_interviews_and_reports() {
    for ncp in NCPS {
        let dir = tempfile::tempdir().unwrap();
        let sim = sim(
            ncp,
            vec![
                SimDevice::temperature_humidity_sensor(TH),
                SimDevice::contact_sensor(DOOR),
            ],
        );
        let settings = random_network(15).unwrap();
        let (host, mut rx) = start(
            &sim,
            ncp,
            StartMode::Normal {
                settings: settings.clone(),
                replace_other: false,
            },
            config(dir.path()),
        )
        .await
        .unwrap();
        let outcome = wait_for(&mut rx, |e| match e {
            HostEvent::Started {
                outcome,
                network,
                adapter,
            } => {
                assert_eq!(network.channel, 15);
                assert_eq!(network.pan_id, settings.pan_id);
                assert_eq!(adapter.kind, kind(ncp).name());
                Some(*outcome)
            }
            _ => None,
        })
        .await;
        assert_eq!(outcome, StartOutcome::Formed);
        let network = sim.snapshot().network.expect("formed");
        assert_eq!(network.network_key, settings.network_key);
        assert_eq!(network.extended_pan_id, settings.extended_pan_id);

        host.permit_join(60, None).await.unwrap();
        assert!(sim.snapshot().permit_join);
        let mut done = Vec::new();
        while done.len() < 2 {
            let device = wait_for(&mut rx, |e| match e {
                HostEvent::InterviewDone { device } => Some(device.clone()),
                _ => None,
            })
            .await;
            done.push(*device);
        }
        let th = done
            .iter()
            .find(|d| d.ieee == TH)
            .expect("sensor interviewed");
        assert_eq!(th.model.as_deref(), Some("SIM-TH1"));
        assert_eq!(th.manufacturer.as_deref(), Some("zigbee-bridge-sim"));
        assert_eq!(th.power_source, Some(3));
        assert_eq!(
            (th.zcl_version, th.stack_version, th.hw_version),
            (Some(8), Some(2), Some(1))
        );
        assert!(th.is_sleepy());
        if matches!(ncp, SimNcp::Ezsp { .. }) {
            // Z-Stack does not report link key exchanges.
            assert!(th.unique_link_key);
        }
        assert_eq!(th.interview.stage, InterviewStage::Done);
        assert!(th
            .endpoint_with(cluster_id::TEMPERATURE_MEASUREMENT)
            .is_some());
        assert!(th
            .reporting
            .iter()
            .any(|r| r.cluster == cluster_id::TEMPERATURE_MEASUREMENT && r.status == 0));

        let snap = sim.snapshot();
        assert!(snap
            .bindings
            .contains(&(TH, 1, cluster_id::TEMPERATURE_MEASUREMENT)));
        assert!(snap.reporting.contains(&(
            TH,
            1,
            cluster_id::TEMPERATURE_MEASUREMENT,
            attr::temperature_measurement::MEASURED_VALUE
        )));
        assert!(
            snap.ias_enrolled.contains(&DOOR),
            "door enrolled in an IAS zone"
        );
        let door = host.device(DOOR).unwrap();
        assert!(door.ias_zone_id.is_some());

        // A temperature change reaches the host as a report.
        assert_eq!(
            sim.set_attribute(
                TH,
                1,
                cluster_id::TEMPERATURE_MEASUREMENT,
                attr::temperature_measurement::MEASURED_VALUE,
                Value::i16(2150)
            ),
            1
        );
        let frame = wait_for(&mut rx, |e| match e {
            HostEvent::Frame { ieee, link, frame }
                if *ieee == TH && link.cluster == cluster_id::TEMPERATURE_MEASUREMENT =>
            {
                assert_eq!(
                    (
                        link.source_endpoint,
                        link.destination_endpoint,
                        link.profile
                    ),
                    (1, 1, 0x0104)
                );
                assert_eq!(link.lqi, 180);
                Some(frame.clone())
            }
            _ => None,
        })
        .await;
        // Report attributes, measured_value, int16, 2150.
        assert_eq!(
            frame.get(2..),
            Some(&[0x0A, 0x00, 0x00, 0x29, 0x66, 0x08][..])
        );
        let value = wait_for(&mut rx, |e| match e {
            HostEvent::Attributes {
                ieee,
                cluster,
                attributes,
                ..
            } if *ieee == TH && *cluster == cluster_id::TEMPERATURE_MEASUREMENT => {
                attributes.first().cloned()
            }
            _ => None,
        })
        .await;
        assert_eq!(
            value,
            (
                attr::temperature_measurement::MEASURED_VALUE,
                Value::i16(2150)
            )
        );

        // Raw ZCL: read attribute 0 with a frame built by the caller; the
        // host writes its own sequence number and returns the reply as is.
        let reply = host
            .zcl_raw(
                TH,
                zigbee_host::RawZcl {
                    endpoint: 1,
                    cluster: cluster_id::TEMPERATURE_MEASUREMENT,
                    frame: vec![0x00, 0x00, 0x00, 0x00, 0x00],
                    profile: None,
                    source_endpoint: 1,
                    response: true,
                    timeout: None,
                },
            )
            .await
            .unwrap()
            .expect("a reply");
        let parsed = zigbee_zcl::Frame::parse(&reply).unwrap();
        assert_eq!(
            zigbee_zcl::Global::decode(parsed.header.command, &parsed.payload).unwrap(),
            zigbee_zcl::Global::ReadAttributesResponse(vec![zigbee_zcl::foundation::ReadRecord {
                attribute: 0,
                status: 0,
                value: Some(Value::i16(2150)),
            }])
        );
        // Without a response, the host waits for APS delivery only.
        let identify = host
            .zcl_raw(
                TH,
                zigbee_host::RawZcl {
                    endpoint: 1,
                    cluster: cluster_id::IDENTIFY,
                    frame: vec![0x11, 0x00, 0x00, 0x05, 0x00],
                    profile: Some(0x0104),
                    source_endpoint: 1,
                    response: false,
                    timeout: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(identify, None);
        // Raw ZDO: a node descriptor request, sequence number placeholder first.
        let nwk = host.device(TH).unwrap().nwk.to_le_bytes();
        let zdo = host
            .zdo_raw(TH, 0x0002, vec![0, nwk[0], nwk[1]], true, None)
            .await
            .unwrap()
            .expect("a ZDO reply");
        assert_eq!(zdo.get(1), Some(&0u8), "status success");
        assert_eq!(zdo.get(2..4), Some(&nwk[..]));

        // A read goes to the device and back.
        let records = host
            .read_attributes(TH, 1, cluster_id::TEMPERATURE_MEASUREMENT, &[0, 0x7777])
            .await
            .unwrap();
        assert_eq!(records[0].value, Some(Value::i16(2150)));
        assert_eq!(records[1].status, zigbee_zcl::status::UNSUPPORTED_ATTRIBUTE);

        // Topology learned from last hops.
        assert!(host
            .topology()
            .iter()
            .any(|(from, to, _)| *from == Some(TH) && *to == Some(host.network().ieee)));

        // The backup carries what a restore needs.
        let backup = host.backup().await.unwrap();
        assert_eq!(backup.network_key.key, settings.network_key);
        assert_eq!(backup.coordinator_ieee, host.network().ieee);
        // deCONZ keeps one global link key: nothing stack-specific.
        if ncp != SimNcp::Deconz {
            let stack = backup.stack_specific.as_ref().expect("stack data");
            assert!(stack.ezsp.is_some() || stack.zstack.is_some());
        }
        assert!(backup.devices.iter().any(|d| d.ieee_address == TH));
        let on_disk =
            Backup::from_json(&std::fs::read_to_string(dir.path().join("backup.json")).unwrap())
                .unwrap();
        assert_eq!(on_disk.pan_id, backup.pan_id);
        host.flush().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resumes_after_restart_and_refuses_another_network() {
    for ncp in NCPS {
        let dir = tempfile::tempdir().unwrap();
        let sim = sim(ncp, vec![SimDevice::plug(PLUG)]);
        let settings = random_network(20).unwrap();
        {
            let (host, mut rx) = start(
                &sim,
                ncp,
                StartMode::Normal {
                    settings: settings.clone(),
                    replace_other: false,
                },
                config(dir.path()),
            )
            .await
            .unwrap();
            host.permit_join(30, None).await.unwrap();
            wait_for(&mut rx, |e| {
                matches!(e, HostEvent::InterviewDone { .. }).then_some(())
            })
            .await;
            // On/off by name through the cluster library.
            host.command(PLUG, 1, cluster_id::ON_OFF, "on", &[])
                .await
                .unwrap();
            let report = wait_for(&mut rx, |e| match e {
                HostEvent::Attributes {
                    cluster,
                    attributes,
                    ..
                } if *cluster == cluster_id::ON_OFF => attributes.first().cloned(),
                _ => None,
            })
            .await;
            assert_eq!(report, (attr::on_off::ON_OFF, Value::bool(true)));
            host.shutdown().await.unwrap();
        }
        // A new host on the same NCP resumes the network and keeps the device.
        let (host, mut rx) = start(
            &sim,
            ncp,
            StartMode::Normal {
                settings: settings.clone(),
                replace_other: false,
            },
            config(dir.path()),
        )
        .await
        .unwrap();
        let outcome = wait_for(&mut rx, |e| match e {
            HostEvent::Started { outcome, .. } => Some(*outcome),
            _ => None,
        })
        .await;
        assert_eq!(outcome, StartOutcome::Resumed);
        let plug = host.device(PLUG).expect("device kept in the store");
        assert!(plug.interview.is_done());
        drop(host);

        // Different settings on the same NCP: refused, nothing changed.
        let other = NetworkSettings {
            pan_id: settings.pan_id ^ 1,
            ..random_network(20).unwrap()
        };
        let err = start(
            &sim,
            ncp,
            StartMode::Normal {
                settings: other,
                replace_other: false,
            },
            config(dir.path()),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HostError::NetworkMismatch { .. }), "{err}");
        assert_eq!(
            sim.snapshot().network.unwrap().network_key,
            settings.network_key
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restores_a_backup_onto_a_new_ncp() {
    for ncp in NCPS {
        let dir = tempfile::tempdir().unwrap();
        let old = sim(ncp, vec![SimDevice::temperature_humidity_sensor(TH)]);
        let settings = random_network(25).unwrap();
        let (host, mut rx) = start(
            &old,
            ncp,
            StartMode::Normal {
                settings: settings.clone(),
                replace_other: false,
            },
            config(dir.path()),
        )
        .await
        .unwrap();
        host.permit_join(30, None).await.unwrap();
        wait_for(&mut rx, |e| {
            matches!(e, HostEvent::InterviewDone { .. }).then_some(())
        })
        .await;
        let backup = host.backup().await.unwrap();
        host.shutdown().await.unwrap();
        drop(host);

        // A replacement NCP with another factory IEEE address.
        let new = Simulator::new(SimConfig {
            ncp,
            ieee: Ieee(0x000D6FFFFE999999),
            ..SimConfig::default()
        });
        if let SimNcp::Ezsp { .. } = ncp {
            // EmberZNet takes the IEEE address once, at the next start.
            let err = start(
                &new,
                ncp,
                StartMode::Restore(Box::new(backup.clone())),
                config(dir.path()),
            )
            .await
            .unwrap_err();
            assert!(matches!(err, HostError::RestartRequired), "{err}");
        }
        let (host, mut rx) = start(
            &new,
            ncp,
            StartMode::Restore(Box::new(backup.clone())),
            config(dir.path()),
        )
        .await
        .unwrap();
        let outcome = wait_for(&mut rx, |e| match e {
            HostEvent::Started { outcome, .. } => Some(*outcome),
            _ => None,
        })
        .await;
        assert_eq!(outcome, StartOutcome::Restored);
        let snap = new.snapshot();
        let network = snap.network.unwrap();
        assert_eq!(snap.ieee, backup.coordinator_ieee);
        assert_eq!(network.network_key, settings.network_key);
        assert_eq!(network.pan_id, settings.pan_id);
        assert_eq!(network.channel, 25);
        if let Some(stack) = backup.stack_specific.clone() {
            assert_eq!(
                Some(network.tc_root),
                stack
                    .ezsp
                    .map(|e| e.hashed_tclk)
                    .or(stack.zstack.map(|z| z.tclk_seed))
            );
        }
        assert!(
            network.frame_counter
                >= backup.network_key.frame_counter
                    + zigbee_host::backup::RESTORE_FRAME_COUNTER_MARGIN
        );
        // The device record survived in the store.
        assert!(host.device(TH).is_some());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn install_code_only_and_legacy_policies() {
    for ncp in NCPS {
        let dir = tempfile::tempdir().unwrap();
        let code =
            zigbee_host::install_code::parse_hex("83FED3407A939723A5C639B26916D505C3B5").unwrap();
        let key = zigbee_host::install_code::link_key(&code).unwrap();
        let mut secure = SimDevice::plug(PLUG);
        secure.install_code_key = Some(key);
        let mut legacy = SimDevice::temperature_humidity_sensor(TH);
        legacy.legacy = true;
        let sim = sim(ncp, vec![secure, legacy]);
        let mut cfg = config(dir.path());
        cfg.join = JoinPolicy {
            install_code_only: true,
            allow_legacy: false,
        };
        cfg.legacy_grace = Duration::from_secs(1);
        let (host, mut rx) = start(
            &sim,
            ncp,
            StartMode::Normal {
                settings: random_network(11).unwrap(),
                replace_other: false,
            },
            cfg,
        )
        .await
        .unwrap();
        if ncp == SimNcp::Deconz {
            // The firmware cannot require install codes or turn away
            // legacy devices: refused, not pretended.
            assert!(matches!(
                host.permit_join(30, None).await,
                Err(HostError::Invalid(_))
            ));
            continue;
        }
        host.permit_join(30, None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(
            sim.snapshot().joined.is_empty(),
            "nothing joins without an install code"
        );
        host.add_install_code(PLUG, &code).await.unwrap();
        let joined = wait_for(&mut rx, |e| match e {
            HostEvent::DeviceJoined { ieee, .. } => Some(*ieee),
            _ => None,
        })
        .await;
        assert_eq!(joined, PLUG);
        assert!(!sim.snapshot().joined.iter().any(|(i, _)| *i == TH));
        drop(host);

        // A legacy device on an open network is removed after the grace period.
        let dir = tempfile::tempdir().unwrap();
        let mut legacy = SimDevice::temperature_humidity_sensor(TH);
        legacy.legacy = true;
        let sim = self::sim(ncp, vec![legacy]);
        let mut cfg = config(dir.path());
        cfg.join = JoinPolicy {
            install_code_only: false,
            allow_legacy: false,
        };
        cfg.legacy_grace = Duration::from_secs(1);
        let (host, mut rx) = start(
            &sim,
            ncp,
            StartMode::Normal {
                settings: random_network(11).unwrap(),
                replace_other: false,
            },
            cfg,
        )
        .await
        .unwrap();
        host.permit_join(30, None).await.unwrap();
        // The host removes it (EZSP), or Z-Stack does and the host hears it
        // leave.
        let removed = wait_for(&mut rx, |e| match e {
            HostEvent::DeviceRemoved { ieee, .. } | HostEvent::DeviceLeft { ieee } => Some(*ieee),
            _ => None,
        })
        .await;
        assert_eq!(removed, TH);
        assert!(host.device(TH).is_none());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_identifies_the_ncp_without_changing_it() {
    for ncp in NCPS {
        let sim = sim(ncp, vec![]);
        let (host_io, ncp_io) = tokio::io::duplex(4096);
        let s = sim.clone();
        let served = tokio::spawn(async move {
            let _ = s.serve(ncp_io).await;
        });
        let started = std::time::Instant::now();
        let id = zigbee_host::probe::probe_stream(host_io, kind(ncp))
            .await
            .unwrap()
            .expect("an NCP");
        // The probe closed its end: the simulator sees the stream end.
        tokio::time::timeout(Duration::from_secs(2), served)
            .await
            .expect("the probe left the port open")
            .unwrap();
        assert!(started.elapsed() < zigbee_host::probe::PROBE_TIMEOUT);
        assert_eq!(id.kind, kind(ncp));
        assert!(id.supported);
        assert_eq!(id.ieee, Some(SimConfig::default().ieee));
        match ncp {
            SimNcp::Ezsp { protocol } => {
                assert_eq!(id.protocol, protocol);
                let firmware = if protocol == 13 { "7.4.4.0" } else { "8.0.2.0" };
                assert_eq!(id.firmware, firmware);
                assert_eq!(id.manufacturer.as_deref(), Some("zigbee-bridge sim"));
            }
            SimNcp::Znp { product } => {
                assert_eq!(id.firmware, "2.7.1.20230507");
                let name = if product == 1 {
                    "Z-Stack 3.x.0"
                } else {
                    "Z-Stack 3.0.x"
                };
                assert_eq!(id.product.as_deref(), Some(name));
            }
            SimNcp::Deconz => {
                assert_eq!(id.firmware, "0x26780700");
                assert_eq!(id.product.as_deref(), Some("ConBee II/RaspBee II"));
            }
        }
        assert!(sim.snapshot().network.is_none());
    }
}
