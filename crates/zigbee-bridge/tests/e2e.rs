//! The whole bridge, end to end: an MQTT broker, the EZSP NCP simulator on
//! TCP, and `zigbee_bridge::run`. A device joins, is interviewed, gets Home
//! Assistant discovery, and its attribute report arrives over MQTT.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde_json::{json, Value as Json};
use tokio::sync::{mpsc, oneshot};
use zigbee_bridge::Config;
use zigbee_sim::{SimConfig, SimDevice, SimNcp, Simulator};
use zigbee_types::Ieee;
use zigbee_zcl::{attr, cluster_id, Value};

const TH: Ieee = Ieee(0x00124B0000000001);
const PLUG: Ieee = Ieee(0x00124B0000000003);

fn free_port() -> SocketAddr {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap()
}

fn start_broker() -> SocketAddr {
    let addr = free_port();
    let mut servers = HashMap::new();
    servers.insert(
        "v4".to_string(),
        rumqttd::ServerSettings {
            name: "v4".into(),
            listen: addr,
            tls: None,
            next_connection_delay_ms: 1,
            connections: rumqttd::ConnectionSettings {
                connection_timeout_ms: 5000,
                max_payload_size: 256 * 1024,
                max_inflight_count: 200,
                auth: None,
                external_auth: None,
                dynamic_filters: true,
            },
        },
    );
    let config = rumqttd::Config {
        id: 0,
        router: rumqttd::RouterConfig {
            max_connections: 16,
            max_outgoing_packet_count: 200,
            max_segment_size: 16 * 1024 * 1024,
            max_segment_count: 10,
            ..Default::default()
        },
        v4: Some(servers),
        ..Default::default()
    };
    std::thread::spawn(move || {
        let mut broker = rumqttd::Broker::new(config);
        let _ = broker.start();
    });
    addr
}

async fn start_sim(ncp: SimNcp) -> (Simulator, SocketAddr) {
    let sim = Simulator::new(SimConfig {
        ncp,
        devices: vec![
            SimDevice::temperature_humidity_sensor(TH),
            SimDevice::plug(PLUG),
        ],
        ..SimConfig::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let s = sim.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let _ = s.serve(stream).await;
        }
    });
    (sim, addr)
}

struct Watcher {
    client: AsyncClient,
    rx: mpsc::UnboundedReceiver<(String, Vec<u8>)>,
    /// Every message so far, so a wait finds one that arrived earlier.
    seen: Vec<(String, Json)>,
}

impl Watcher {
    async fn connect(broker: SocketAddr) -> Watcher {
        let mut options = MqttOptions::new("e2e-watcher", broker.ip().to_string(), broker.port());
        options.set_keep_alive(Duration::from_secs(10));
        options.set_max_packet_size(256 * 1024, 256 * 1024);
        let (client, mut event_loop) = AsyncClient::new(options, 100);
        let (tx, rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = oneshot::channel();
        let mut ready = Some(ready_tx);
        tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::SubAck(_))) => {
                        if let Some(r) = ready.take() {
                            let _ = r.send(());
                        }
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        let _ = tx.send((p.topic.clone(), p.payload.to_vec()));
                    }
                    Ok(_) => {}
                    Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
                }
            }
        });
        client.subscribe("#", QoS::AtLeastOnce).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), ready_rx)
            .await
            .expect("subscribed")
            .unwrap();
        Watcher {
            client,
            rx,
            seen: Vec::new(),
        }
    }

    /// Wait for a message on `topic` whose JSON satisfies `pred`, whether it
    /// already arrived or arrives within 30 seconds.
    async fn wait(&mut self, topic: &str, mut pred: impl FnMut(&Json) -> bool) -> Json {
        if let Some((_, v)) = self.seen.iter().find(|(t, v)| t == topic && pred(v)) {
            return v.clone();
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let (t, payload) = tokio::time::timeout_at(deadline, self.rx.recv())
                .await
                .unwrap_or_else(|_| panic!("timed out waiting on {topic}"))
                .expect("watcher stopped");
            let value: Json = serde_json::from_slice(&payload).unwrap_or(Json::Null);
            self.seen.push((t.clone(), value.clone()));
            if t == topic && pred(&value) {
                return value;
            }
        }
    }

    async fn request(&mut self, name: &str, body: Json) -> Json {
        self.client
            .publish(
                format!("zigbee/bridge/request/{name}"),
                QoS::AtLeastOnce,
                false,
                serde_json::to_vec(&body).unwrap(),
            )
            .await
            .unwrap();
        let transaction = body["transaction"].clone();
        self.wait(&format!("zigbee/bridge/response/{name}"), |v| {
            v["transaction"] == transaction
        })
        .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ezsp_device_joins_is_interviewed_and_reports_over_mqtt() {
    end_to_end(SimNcp::Ezsp { protocol: 13 }, "ezsp", "7.4.4.0").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn znp_device_joins_is_interviewed_and_reports_over_mqtt() {
    end_to_end(SimNcp::Znp { product: 1 }, "znp", "2.7.1.20230507").await;
}

/// The bridge detects the coordinator family (the configuration leaves
/// `serial.adapter` at `auto`), forms, admits and interviews devices, and
/// carries reports and requests over MQTT.
async fn end_to_end(sim_ncp: SimNcp, kind: &str, firmware: &str) {
    let broker = start_broker();
    let (sim, ncp) = start_sim(sim_ncp).await;
    let data = tempfile::tempdir().unwrap();
    let config = Config::from_toml(&format!(
        "[serial]\nport = \"tcp://{ncp}\"\n[mqtt]\nhost = \"{}\"\nport = {}\n[network]\nchannel = 20\n[data]\ndir = \"{}\"\n",
        broker.ip(),
        broker.port(),
        data.path().display()
    ))
    .unwrap();
    // The broker thread needs a moment to listen.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut mqtt = Watcher::connect(broker).await;

    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let bridge = tokio::spawn(zigbee_bridge::run(config, async move {
        let _ = stop_rx.await;
    }));

    mqtt.wait("zigbee/bridge/state", |v| v["state"] == "online")
        .await;
    let info = mqtt
        .wait("zigbee/bridge/info", |v| v["network"]["channel"] == 20)
        .await;
    assert_eq!(info["coordinator"]["type"], kind);
    assert_eq!(info["coordinator"]["firmware"], firmware);

    let response = mqtt
        .request("permit_join", json!({"time": 60, "transaction": "t1"}))
        .await;
    assert_eq!(response["status"], "ok", "{response}");

    let th_name = TH.to_prefixed_hex();
    let interviewed = mqtt
        .wait("zigbee/bridge/event", |v| {
            v["type"] == "device_interview"
                && v["data"]["status"] == "successful"
                && v["data"]["ieee_address"] == th_name
        })
        .await;
    assert_eq!(interviewed["data"]["model"], "SIM-TH1");

    let discovery = mqtt
        .wait(
            "homeassistant/sensor/00124b0000000001/temperature/config",
            |v| v.is_object(),
        )
        .await;
    assert_eq!(discovery["state_topic"], format!("zigbee/{th_name}"));
    assert_eq!(discovery["device_class"], "temperature");
    assert_eq!(discovery["device"]["model"], "SIM-TH1");

    // The simulated sensor reports a new temperature, per the reporting the
    // bridge configured during the interview.
    let mut reports = 0;
    for _ in 0..50 {
        reports = sim.set_attribute(
            TH,
            1,
            cluster_id::TEMPERATURE_MEASUREMENT,
            attr::temperature_measurement::MEASURED_VALUE,
            Value::i16(2150),
        );
        if reports > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        reports, 1,
        "the sensor was configured to report temperature"
    );
    // The raw report is published first, then the device's state.
    let raw = mqtt
        .wait(&format!("zigbee/{th_name}/report"), |v| {
            v["cluster"] == "temperature_measurement"
        })
        .await;
    assert_eq!(raw["attributes"]["measured_value"], 2150);
    let state = mqtt
        .wait(&format!("zigbee/{th_name}"), |v| {
            v["temperature"] == json!(21.5)
        })
        .await;
    assert!(state["linkquality"].is_number());

    // Commands: switch the plug on from MQTT.
    let plug_name = PLUG.to_prefixed_hex();
    mqtt.wait("zigbee/bridge/event", |v| {
        v["type"] == "device_interview"
            && v["data"]["status"] == "successful"
            && v["data"]["ieee_address"] == plug_name
    })
    .await;
    mqtt.client
        .publish(
            format!("zigbee/{plug_name}/set"),
            QoS::AtLeastOnce,
            false,
            r#"{"state":"ON"}"#,
        )
        .await
        .unwrap();
    mqtt.wait(&format!("zigbee/{plug_name}"), |v| v["state"] == "ON")
        .await;

    // Rename, then the topic follows the name.
    let renamed = mqtt
        .request(
            "rename",
            json!({"from": th_name, "to": "porch", "transaction": 2}),
        )
        .await;
    assert_eq!(renamed["status"], "ok", "{renamed}");
    sim.set_attribute(
        TH,
        1,
        cluster_id::RELATIVE_HUMIDITY_MEASUREMENT,
        attr::relative_humidity_measurement::MEASURED_VALUE,
        Value::u16(5123),
    );
    let porch = mqtt
        .wait("zigbee/porch", |v| v["humidity"] == json!(51.23))
        .await;
    assert_eq!(porch["temperature"], json!(21.5));

    // Backups go to disk, and keys never go to MQTT.
    let backup = mqtt.request("backup", json!({"transaction": 3})).await;
    assert_eq!(backup["status"], "ok", "{backup}");
    let key = sim.snapshot().network.unwrap().network_key.to_hex();
    assert!(!backup.to_string().contains(&key));
    let on_disk = std::fs::read_to_string(data.path().join("backup.json")).unwrap();
    assert!(on_disk.contains(&key));

    let topology = mqtt.request("topology", json!({"transaction": 4})).await;
    assert!(topology["data"]["links"]
        .as_array()
        .is_some_and(|l| !l.is_empty()));

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), bridge)
        .await
        .expect("bridge stopped")
        .unwrap()
        .unwrap();
    let store = std::fs::read_to_string(data.path().join("devices.json")).unwrap();
    assert!(store.contains("SIM-TH1") && store.contains("porch"));
}
